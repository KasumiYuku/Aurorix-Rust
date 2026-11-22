use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use parking_lot::Mutex;
use skia_safe::Canvas;
use tokio::sync::{oneshot, Notify};

use crate::backend::BackendHandle;
use crate::context::RenderContext;
use crate::draw::RenderRequest;
use crate::error::RenderError;
use crate::plan;
use crate::pool::{RenderPool, RenderPoolConfig};
use crate::task::{RenderJob, RenderOutput, RenderPriority, RenderTask, RenderTicket};

const DEFAULT_FONT_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerConfig {
    pub workers: usize,
    pub queue_capacity: usize,
    pub max_in_flight_bytes: u64,
    pub task_timeout: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        plan::detect_scheduler_config(plan::probe_cpus(), plan::probe_memory_bytes())
    }
}

impl SchedulerConfig {
    pub fn texture_capacity(&self) -> u64 {
        self.max_in_flight_bytes / 2
    }

    fn normalized(&self) -> Self {
        Self {
            workers: self.workers.max(1),
            queue_capacity: self.queue_capacity.max(1),
            ..*self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchedulerMetrics {
    pub submitted: u64,
    pub completed: u64,
    pub failed: u64,
    pub rejected: u64,
    pub in_flight: u64,
    pub queued: u64,
    pub workers: u64,
    pub reserved_bytes: u64,
}
#[derive(Default)]
struct Counters {
    submitted: AtomicU64,
    completed: AtomicU64,
    failed: AtomicU64,
    rejected: AtomicU64,
    in_flight: AtomicU64,
    reserved_bytes: AtomicU64,
    next_ticket: AtomicU64,
}

struct QueueEntry {
    ticket: u64,
    task: RenderTask,
    job: RenderJob,
    respond: oneshot::Sender<Result<RenderOutput, RenderError>>,
}

struct Inner {
    config: SchedulerConfig,
    backend: BackendHandle,
    context: Arc<RenderContext>,
    pool: Arc<RenderPool>,
    queue: Mutex<VecDeque<QueueEntry>>,
    wake: Notify,
    drain: Notify,
    closed: AtomicBool,
    workers: AtomicUsize,
    spawned: AtomicUsize,
    counters: Counters,
}

struct Reservation {
    inner: Arc<Inner>,
    bytes: u64,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.inner
            .counters
            .reserved_bytes
            .fetch_sub(self.bytes, Ordering::SeqCst);
    }
}

impl Inner {
    fn enqueue(&self, entry: QueueEntry) -> Result<(), RenderError> {
        let estimated = entry.task.estimated_bytes;
        let capacity = crate::settings::effective_queue(self.config.queue_capacity);
        let max_in_flight = crate::settings::render_budget_bytes()
            .unwrap_or(self.config.max_in_flight_bytes);
        let mut queue = self.queue.lock();
        if queue.len() >= capacity {
            return Err(RenderError::QueueFull);
        }
        let reserved = self.counters.reserved_bytes.load(Ordering::SeqCst);
        if reserved.saturating_add(estimated) > max_in_flight {
            return Err(RenderError::BytesBudgetExceeded);
        }
        self.counters
            .reserved_bytes
            .fetch_add(estimated, Ordering::SeqCst);
        let position = queue
            .iter()
            .position(|queued| queued.task.priority < entry.task.priority)
            .unwrap_or(queue.len());
        queue.insert(position, entry);
        drop(queue);
        self.wake.notify_one();
        Ok(())
    }

    async fn dequeue(&self) -> Option<QueueEntry> {
        loop {
            let notified = self.wake.notified();
            {
                let mut queue = self.queue.lock();
                if self.spawned.load(Ordering::SeqCst) > self.workers.load(Ordering::SeqCst) {
                    return None;
                }
                if let Some(entry) = queue.pop_front() {
                    return Some(entry);
                }
                if self.closed.load(Ordering::SeqCst) {
                    return None;
                }
            }
            notified.await;
        }
    }

    async fn run_job(
        self: &Arc<Self>,
        priority: RenderPriority,
        job: RenderJob,
        reserved: u64,
    ) -> Result<Bytes, RenderError> {
        let context = Arc::clone(&self.context);
        let reservation = Reservation {
            inner: Arc::clone(self),
            bytes: reserved,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let ticket = self.pool.submit(
            priority,
            Box::new(move || {
                let _reservation = reservation;
                if flag.load(Ordering::SeqCst) {
                    return Err(RenderError::Cancelled);
                }
                job(&context)
            }),
        )?;
        match tokio::time::timeout(self.config.task_timeout, ticket.wait()).await {
            Ok(result) => result,
            Err(_) => {
                cancelled.store(true, Ordering::SeqCst);
                Err(RenderError::Timeout)
            }
        }
    }

    async fn execute(self: &Arc<Self>, entry: QueueEntry) {
        let QueueEntry {
            ticket,
            task,
            job,
            respond,
        } = entry;
        self.counters.in_flight.fetch_add(1, Ordering::SeqCst);
        let started = Instant::now();
        let produced = self.run_job(task.priority, job, task.estimated_bytes).await;
        let elapsed = started.elapsed();
        self.counters.in_flight.fetch_sub(1, Ordering::SeqCst);
        let outcome = match produced {
            Ok(bytes) => {
                self.counters.completed.fetch_add(1, Ordering::SeqCst);
                Ok(RenderOutput {
                    ticket,
                    bytes,
                    elapsed,
                })
            }
            Err(error) => {
                self.counters.failed.fetch_add(1, Ordering::SeqCst);
                Err(error)
            }
        };
        let _ = respond.send(outcome);
        self.drain.notify_one();
    }

    fn ensure_workers(self: &Arc<Self>) {
        let target = crate::settings::effective_concurrency(self.config.workers);
        self.workers.store(target, Ordering::SeqCst);
        while self.spawned.load(Ordering::SeqCst) < target {
            self.spawned.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(worker(Arc::clone(self)));
        }
        if self.spawned.load(Ordering::SeqCst) > target {
            self.wake.notify_waiters();
        }
    }
}

async fn worker(inner: Arc<Inner>) {
    while let Some(entry) = inner.dequeue().await {
        inner.execute(entry).await;
    }
    inner.spawned.fetch_sub(1, Ordering::SeqCst);
}

pub struct RenderScheduler {
    inner: Arc<Inner>,
}

impl RenderScheduler {
    pub fn new(config: SchedulerConfig, backend: BackendHandle) -> Arc<Self> {
        Self::try_new(config, backend)
            .unwrap_or_else(|error| panic!("无法初始化渲染调度器: {error}"))
    }

    pub fn try_new(
        config: SchedulerConfig,
        backend: BackendHandle,
    ) -> Result<Arc<Self>, RenderError> {
        let config = config.normalized();
        crate::memory::configure_caches();
        let context = RenderContext::new(
            Arc::clone(&backend),
            DEFAULT_FONT_CAPACITY,
            config.texture_capacity(),
        );
        let pool_config = RenderPoolConfig {
            threads: config.workers.max(1),
            queue_capacity: plan::detect_queue_capacity(config.workers),
        };
        let pool = RenderPool::with_config(pool_config)?;
        let inner = Arc::new(Inner {
            config,
            backend,
            context,
            pool,
            queue: Mutex::new(VecDeque::new()),
            wake: Notify::new(),
            drain: Notify::new(),
            closed: AtomicBool::new(false),
            workers: AtomicUsize::new(0),
            spawned: AtomicUsize::new(0),
            counters: Counters::default(),
        });
        inner.ensure_workers();
        Ok(Arc::new(Self { inner }))
    }

    pub async fn submit(
        &self,
        task: RenderTask,
        job: RenderJob,
    ) -> Result<RenderHandle, RenderError> {
        if self.inner.closed.load(Ordering::SeqCst) {
            self.inner.counters.rejected.fetch_add(1, Ordering::SeqCst);
            return Err(RenderError::Shutdown);
        }
        self.inner.ensure_workers();
        let ticket = self
            .inner
            .counters
            .next_ticket
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        let (respond, receiver) = oneshot::channel();
        let entry = QueueEntry {
            ticket,
            task,
            job,
            respond,
        };
        if let Err(error) = self.inner.enqueue(entry) {
            self.inner.counters.rejected.fetch_add(1, Ordering::SeqCst);
            return Err(error);
        }
        self.inner.counters.submitted.fetch_add(1, Ordering::SeqCst);
        Ok(RenderHandle {
            ticket: RenderTicket::new(ticket),
            receiver,
        })
    }

    pub async fn submit_render<F>(
        &self,
        task: RenderTask,
        request: RenderRequest,
        painter: F,
    ) -> Result<RenderHandle, RenderError>
    where
        F: FnOnce(&Canvas, &RenderContext) + Send + 'static,
    {
        let job: RenderJob = Box::new(move |context| context.draw(request, painter));
        self.submit(task, job).await
    }

    pub async fn shutdown(&self) {
        if !self.inner.closed.swap(true, Ordering::SeqCst) {
            self.inner.wake.notify_waiters();
        }
        loop {
            {
                let queue = self.inner.queue.lock();
                let idle = self.inner.counters.in_flight.load(Ordering::SeqCst) == 0;
                if queue.is_empty() && idle {
                    break;
                }
            }
            self.inner.drain.notified().await;
        }
        self.inner.pool.shutdown();
    }

    pub fn metrics(&self) -> SchedulerMetrics {
        let queued = self.inner.queue.lock().len() as u64;
        SchedulerMetrics {
            submitted: self.inner.counters.submitted.load(Ordering::SeqCst),
            completed: self.inner.counters.completed.load(Ordering::SeqCst),
            failed: self.inner.counters.failed.load(Ordering::SeqCst),
            rejected: self.inner.counters.rejected.load(Ordering::SeqCst),
            in_flight: self.inner.counters.in_flight.load(Ordering::SeqCst),
            queued,
            workers: self.inner.workers.load(Ordering::SeqCst) as u64,
            reserved_bytes: self.inner.counters.reserved_bytes.load(Ordering::SeqCst),
        }
    }

    pub fn backend(&self) -> &BackendHandle {
        &self.inner.backend
    }

    pub fn context(&self) -> &Arc<RenderContext> {
        &self.inner.context
    }

    pub fn pool(&self) -> &Arc<RenderPool> {
        &self.inner.pool
    }
    pub fn config(&self) -> &SchedulerConfig {
        &self.inner.config
    }

    pub fn workers(&self) -> usize {
        self.inner.workers.load(Ordering::SeqCst)
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }
}

pub struct RenderHandle {
    ticket: RenderTicket,
    receiver: oneshot::Receiver<Result<RenderOutput, RenderError>>,
}

impl RenderHandle {
    pub fn ticket(&self) -> u64 {
        self.ticket.id()
    }

    pub fn ticket_slot(&self) -> RenderTicket {
        self.ticket
    }

    pub async fn wait(self) -> Result<RenderOutput, RenderError> {
        match self.receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(RenderError::Backend(String::from("渲染结果通道已关闭"))),
        }
    }
}

impl std::fmt::Debug for RenderHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RenderHandle")
            .field("ticket", &self.ticket.id())
            .finish_non_exhaustive()
    }
}
