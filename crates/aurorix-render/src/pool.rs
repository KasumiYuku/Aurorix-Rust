use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use bytes::Bytes;
use parking_lot::{Condvar, Mutex};
use skia_safe::{surfaces, AlphaType, ColorType, ImageInfo, Surface};

use crate::gpu;
use tokio::sync::oneshot;

use crate::error::RenderError;
use crate::plan;
use crate::task::RenderPriority;

const MAX_POOL_THREAD_CAP: usize = 64;
const WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;

pub type PoolJob = Box<dyn FnOnce() -> Result<Bytes, RenderError> + Send + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderPoolConfig {
    pub threads: usize,
    pub queue_capacity: usize,
}

impl Default for RenderPoolConfig {
    fn default() -> Self {
        plan::current_plan().pool
    }
}

impl RenderPoolConfig {
    pub fn new(threads: usize) -> Self {
        Self {
            threads: threads.max(1),
            ..Self::default()
        }
    }

    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    pub fn with_queue_capacity(mut self, queue_capacity: usize) -> Self {
        self.queue_capacity = queue_capacity.max(1);
        self
    }

    fn normalized(self) -> Self {
        Self {
            threads: self.threads.clamp(1, MAX_POOL_THREAD_CAP),
            queue_capacity: self.queue_capacity.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoolStats {
    pub spawned_threads: u64,
    pub live_threads: u64,
    pub dispatched: u64,
    pub completed: u64,
    pub failed: u64,
    pub rejected: u64,
    pub queued: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SurfaceReuseStats {
    pub hits: u64,
    pub misses: u64,
    pub allocations: u64,
}

impl SurfaceReuseStats {
    pub fn total(&self) -> u64 {
        self.hits + self.misses
    }

    pub fn hit_rate(&self) -> f32 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            self.hits as f32 / total as f32
        }
    }
}

static REUSE_HITS: AtomicU64 = AtomicU64::new(0);
static REUSE_MISSES: AtomicU64 = AtomicU64::new(0);
static REUSE_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

pub fn surface_reuse_stats() -> SurfaceReuseStats {
    SurfaceReuseStats {
        hits: REUSE_HITS.load(Ordering::SeqCst),
        misses: REUSE_MISSES.load(Ordering::SeqCst),
        allocations: REUSE_ALLOCATIONS.load(Ordering::SeqCst),
    }
}

pub fn reset_surface_reuse_stats() {
    REUSE_HITS.store(0, Ordering::SeqCst);
    REUSE_MISSES.store(0, Ordering::SeqCst);
    REUSE_ALLOCATIONS.store(0, Ordering::SeqCst);
}

struct BufferSlot {
    width: i32,
    height: i32,
    bytes: Vec<u8>,
}

impl BufferSlot {
    fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            bytes: Vec::new(),
        }
    }

    fn ensure(&mut self, need: usize) -> bool {
        let grew = self.bytes.capacity() < need;
        if self.bytes.len() < need {
            self.bytes.resize(need, 0);
        }
        grew
    }

    fn bytes(&self) -> u64 {
        self.bytes.capacity() as u64
    }
}

struct SurfaceCache {
    slots: Vec<BufferSlot>,
    hits: u64,
    misses: u64,
    allocations: u64,
}

impl SurfaceCache {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            hits: 0,
            misses: 0,
            allocations: 0,
        }
    }

    fn stats(&self) -> SurfaceReuseStats {
        SurfaceReuseStats {
            hits: self.hits,
            misses: self.misses,
            allocations: self.allocations,
        }
    }

    fn sync_epoch(&mut self) {
        let epoch = crate::memory::surface_epoch();
        if SEEN_EPOCH.with(Cell::get) == epoch {
            return;
        }
        self.slots.clear();
        SEEN_EPOCH.with(|cell| cell.set(epoch));
    }

    fn acquire(&mut self, width: i32, height: i32, need: usize) -> usize {
        let limit = crate::memory::surface_cache_limit();
        self.acquire_with_limit(limit, width, height, need)
    }

    fn acquire_with_limit(&mut self, limit: usize, width: i32, height: i32, need: usize) -> usize {
        if limit == 0 {
            REUSE_MISSES.fetch_add(1, Ordering::SeqCst);
            self.misses += 1;
            self.slots.clear();
            let mut slot = BufferSlot::new(width, height);
            self.account(slot.ensure(need));
            self.slots.push(slot);
            return 0;
        }
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.width == width && slot.height == height)
        {
            REUSE_HITS.fetch_add(1, Ordering::SeqCst);
            self.hits += 1;
            let grew = self.slots[index].ensure(need);
            self.account(grew);
            return index;
        }
        REUSE_MISSES.fetch_add(1, Ordering::SeqCst);
        self.misses += 1;
        if self.slots.len() >= limit {
            let victim = self
                .slots
                .iter()
                .enumerate()
                .min_by_key(|(_, slot)| slot.bytes())
                .map(|(index, _)| index)
                .unwrap_or(0);
            let mut slot = self.slots.remove(victim);
            slot.width = width;
            slot.height = height;
            self.account(slot.ensure(need));
            self.slots.push(slot);
        } else {
            let mut slot = BufferSlot::new(width, height);
            self.account(slot.ensure(need));
            self.slots.push(slot);
        }
        self.trim(need);
        self.slots.len() - 1
    }

    fn account(&mut self, grew: bool) {
        if grew {
            REUSE_ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
            self.allocations += 1;
        }
    }
    fn trim(&mut self, keep: usize) {
        let budget = plan::surface_cache_budget() as usize;
        let mut total: usize = self.slots.iter().map(|slot| slot.bytes.len()).sum();
        while total > budget && self.slots.len() > 1 {
            let victim = self
                .slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| slot.bytes.len() != keep)
                .min_by_key(|(_, slot)| slot.bytes.len())
                .map(|(index, _)| index);
            let Some(index) = victim else {
                break;
            };
            let removed = self.slots.remove(index);
            total = total.saturating_sub(removed.bytes.len());
        }
    }
}

pub fn thread_reuse_stats() -> SurfaceReuseStats {
    SURFACE_CACHE
        .try_with(|cell| cell.try_borrow().map(|cache| cache.stats()).ok())
        .ok()
        .flatten()
        .unwrap_or_default()
}

thread_local! {
    static SURFACE_CACHE: RefCell<SurfaceCache> = RefCell::new(SurfaceCache::new());
    static SEEN_EPOCH: Cell<u64> = const { Cell::new(u64::MAX) };
    static POOL_WORKER: Cell<bool> = const { Cell::new(false) };
}

pub fn is_pool_worker() -> bool {
    POOL_WORKER.with(Cell::get)
}

fn image_info(width: i32, height: i32) -> Result<(ImageInfo, usize, usize), RenderError> {
    if width <= 0 || height <= 0 {
        return Err(RenderError::Backend(format!(
            "画布尺寸非法: {width}x{height}"
        )));
    }
    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let row_bytes = info.min_row_bytes();
    let size = info.compute_byte_size(row_bytes);
    Ok((info, row_bytes, size))
}

pub fn with_surface<R>(
    width: i32,
    height: i32,
    draw: impl FnOnce(&mut Surface) -> Result<R, RenderError>,
) -> Result<R, RenderError> {
    with_buffer(width, height, |info, row_bytes, pixels| {
        let mut surface = surfaces::wrap_pixels(info, pixels, row_bytes, None)
            .ok_or_else(|| buffer_error(info))?;
        surface.canvas().clear(crate::draw::Color::TRANSPARENT.to_skia());
        draw(&mut surface)
    })
}

pub fn paint<D, F, R>(width: i32, height: i32, draw: D, finish: F) -> Result<R, RenderError>
where
    D: FnOnce(&mut Surface) -> Result<(), RenderError>,
    F: FnOnce(&mut [u8], usize) -> Result<R, RenderError>,
{
    crate::memory::before_render();
    let _active = crate::memory::enter_render();
    with_buffer(width, height, |info, row_bytes, pixels| {
        paint_into(info, row_bytes, pixels, draw, finish)
    })
}

fn with_buffer<R>(
    width: i32,
    height: i32,
    body: impl FnOnce(&ImageInfo, usize, &mut [u8]) -> Result<R, RenderError>,
) -> Result<R, RenderError> {
    let (info, row_bytes, size) = image_info(width, height)?;
    let trimmable =
        crate::memory::surface_cache_limit() > 0 && size as u64 <= plan::surface_cache_budget();
    let cache_available = trimmable
        && SURFACE_CACHE
            .try_with(|cell| cell.try_borrow_mut().is_ok())
            .unwrap_or(false);
    if !cache_available {
        REUSE_MISSES.fetch_add(1, Ordering::SeqCst);
        REUSE_ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        let mut owned = vec![0u8; size];
        return body(&info, row_bytes, &mut owned);
    }
    SURFACE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        cache.sync_epoch();
        let index = cache.acquire(width, height, size);
        let bytes = &mut cache.slots[index].bytes;
        body(&info, row_bytes, bytes)
    })
}

fn buffer_error(info: &ImageInfo) -> RenderError {
    RenderError::Backend(format!(
        "无法包装 {}x{} 的可复用像素缓冲",
        info.width(),
        info.height()
    ))
}

fn paint_into<D, F, R>(
    info: &ImageInfo,
    row_bytes: usize,
    pixels: &mut [u8],
    draw: D,
    finish: F,
) -> Result<R, RenderError>
where
    D: FnOnce(&mut Surface) -> Result<(), RenderError>,
    F: FnOnce(&mut [u8], usize) -> Result<R, RenderError>,
{
    if gpu::is_usable() {
        match gpu::ensure_ready() {
            Err(reason) => gpu::note_failure(&reason),
            Ok(()) => {
                let painted =
                    gpu::render_into(info, pixels, row_bytes, draw).map_err(|reason| {
                        gpu::note_failure(&reason);
                        RenderError::Backend(format!("GPU 渲染失败: {reason}"))
                    })?;
                painted?;
                return finish(pixels, row_bytes);
            }
        }
    }
    let mut surface =
        surfaces::wrap_pixels(info, pixels, row_bytes, None).ok_or_else(|| buffer_error(info))?;
    draw(&mut surface)?;
    finish(pixels, row_bytes)
}

struct QueuedJob {
    priority: RenderPriority,
    job: PoolJob,
    respond: oneshot::Sender<Result<Bytes, RenderError>>,
}

#[derive(Default)]
struct PoolState {
    queue: VecDeque<QueuedJob>,
}

pub struct RenderPool {
    config: RenderPoolConfig,
    state: Mutex<PoolState>,
    signal: Condvar,
    closed: AtomicBool,
    live: AtomicUsize,
    spawned: AtomicU64,
    dispatched: AtomicU64,
    completed: AtomicU64,
    failed: AtomicU64,
    rejected: AtomicU64,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl RenderPool {
    pub fn new() -> Result<Arc<Self>, RenderError> {
        Self::with_config(RenderPoolConfig::default())
    }

    pub fn with_config(config: RenderPoolConfig) -> Result<Arc<Self>, RenderError> {
        let config = config.normalized();
        let pool = Arc::new(Self {
            config,
            state: Mutex::new(PoolState::default()),
            signal: Condvar::new(),
            closed: AtomicBool::new(false),
            live: AtomicUsize::new(0),
            spawned: AtomicU64::new(0),
            dispatched: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            handles: Mutex::new(Vec::new()),
        });
        let mut handles = Vec::with_capacity(config.threads);
        for index in 0..config.threads {
            let worker_pool = Arc::clone(&pool);
            let builder = thread::Builder::new()
                .name(format!("aurorix-render-{index}"))
                .stack_size(WORKER_STACK_BYTES);
            match builder.spawn(move || worker(worker_pool)) {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    pool.handles.lock().extend(handles);
                    pool.shutdown();
                    return Err(RenderError::Backend(format!(
                        "无法创建渲染线程池工作线程: {error}"
                    )));
                }
            }
        }
        let spawned = handles.len() as u64;
        pool.handles.lock().extend(handles);
        pool.spawned.store(spawned, Ordering::SeqCst);
        Ok(pool)
    }

    pub fn config(&self) -> RenderPoolConfig {
        self.config
    }

    pub fn thread_count(&self) -> usize {
        self.config.threads
    }

    pub fn live_threads(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub fn queue_len(&self) -> usize {
        self.state.lock().queue.len()
    }

    pub fn submit(
        &self,
        priority: RenderPriority,
        job: PoolJob,
    ) -> Result<PoolTicket, RenderError> {
        if self.closed.load(Ordering::SeqCst) {
            self.rejected.fetch_add(1, Ordering::SeqCst);
            return Err(RenderError::Shutdown);
        }
        let (respond, receiver) = oneshot::channel();
        {
            let mut state = self.state.lock();
            let capacity = crate::settings::effective_queue(self.config.queue_capacity);
            if state.queue.len() >= capacity {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                return Err(RenderError::enqueue_full(capacity));
            }
            let entry = QueuedJob {
                priority,
                job,
                respond,
            };
            let position = state
                .queue
                .iter()
                .position(|queued| queued.priority < priority)
                .unwrap_or(state.queue.len());
            state.queue.insert(position, entry);
            self.dispatched.fetch_add(1, Ordering::SeqCst);
            self.signal.notify_one();
        }
        Ok(PoolTicket { receiver })
    }

    pub fn execute(
        &self,
        priority: RenderPriority,
        job: impl FnOnce() -> Result<Bytes, RenderError> + Send + 'static,
    ) -> Result<Bytes, RenderError> {
        self.submit(priority, Box::new(job))?.wait_blocking()
    }

    pub fn stats(&self) -> PoolStats {
        PoolStats {
            spawned_threads: self.spawned.load(Ordering::SeqCst),
            live_threads: self.live.load(Ordering::SeqCst) as u64,
            dispatched: self.dispatched.load(Ordering::SeqCst),
            completed: self.completed.load(Ordering::SeqCst),
            failed: self.failed.load(Ordering::SeqCst),
            rejected: self.rejected.load(Ordering::SeqCst),
            queued: self.state.lock().queue.len() as u64,
        }
    }

    pub fn surface_reuse_stats(&self) -> SurfaceReuseStats {
        surface_reuse_stats()
    }

    pub fn shutdown(&self) {
        let handles = {
            let guard = self.state.lock();
            if !self.closed.swap(true, Ordering::SeqCst) {
                self.signal.notify_all();
            }
            drop(guard);
            let mut handles = self.handles.lock();
            std::mem::take(&mut *handles)
        };
        if handles.is_empty() {
            return;
        }
        if is_pool_worker() {
            return;
        }
        for handle in handles {
            let _ = handle.join();
        }
    }
}

impl Drop for RenderPool {
    fn drop(&mut self) {
        let handles = {
            let guard = self.state.lock();
            if !self.closed.swap(true, Ordering::SeqCst) {
                self.signal.notify_all();
            }
            drop(guard);
            let mut handles = self.handles.lock();
            std::mem::take(&mut *handles)
        };
        if is_pool_worker() {
            return;
        }
        for handle in handles {
            let _ = handle.join();
        }
    }
}

impl std::fmt::Debug for RenderPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RenderPool")
            .field("threads", &self.config.threads)
            .field("queue_capacity", &self.config.queue_capacity)
            .field("live_threads", &self.live_threads())
            .field("closed", &self.is_closed())
            .finish()
    }
}

fn worker(pool: Arc<RenderPool>) {
    POOL_WORKER.with(|flag| flag.set(true));
    pool.live.fetch_add(1, Ordering::SeqCst);
    loop {
        let entry = {
            let mut state = pool.state.lock();
            loop {
                if let Some(entry) = state.queue.pop_front() {
                    break Some(entry);
                }
                if pool.closed.load(Ordering::SeqCst) {
                    break None;
                }
                pool.signal.wait(&mut state);
            }
        };
        let Some(QueuedJob { job, respond, .. }) = entry else {
            break;
        };
        let outcome = match catch_unwind(AssertUnwindSafe(job)) {
            Ok(result) => result,
            Err(_) => Err(RenderError::Panicked),
        };
        match &outcome {
            Ok(_) => {
                pool.completed.fetch_add(1, Ordering::SeqCst);
            }
            Err(_) => {
                pool.failed.fetch_add(1, Ordering::SeqCst);
            }
        }
        let _ = respond.send(outcome);
        crate::memory::after_render();
    }
    pool.live.fetch_sub(1, Ordering::SeqCst);
}

pub struct PoolTicket {
    receiver: oneshot::Receiver<Result<Bytes, RenderError>>,
}

impl PoolTicket {
    pub async fn wait(self) -> Result<Bytes, RenderError> {
        match self.receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(RenderError::Backend(String::from(
                "渲染线程池结果通道已关闭",
            ))),
        }
    }

    pub fn wait_blocking(self) -> Result<Bytes, RenderError> {
        match self.receiver.blocking_recv() {
            Ok(outcome) => outcome,
            Err(_) => Err(RenderError::Backend(String::from(
                "渲染线程池结果通道已关闭",
            ))),
        }
    }

    pub fn receiver(self) -> oneshot::Receiver<Result<Bytes, RenderError>> {
        self.receiver
    }
}

impl std::fmt::Debug for PoolTicket {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PoolTicket").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_surface_limit_keeps_only_the_current_canvas() {
        let mut cache = SurfaceCache::new();
        assert_eq!(cache.acquire_with_limit(0, 32, 32, 4096), 0);
        assert_eq!(cache.slots.len(), 1, "零缓存档位也必须有可用画布");
        assert_eq!(cache.acquire_with_limit(0, 64, 48, 12288), 0);
        assert_eq!(cache.slots.len(), 1, "零缓存档位最多留下一张画布");
        assert_eq!((cache.slots[0].width, cache.slots[0].height), (64, 48));
    }

    #[test]
    fn cached_canvas_is_reused_within_one_epoch() {
        let mut cache = SurfaceCache::new();
        let limit = crate::memory::surface_cache_limit().max(1);
        let first = cache.acquire_with_limit(limit, 32, 32, 4096);
        let second = cache.acquire_with_limit(limit, 32, 32, 4096);
        assert_eq!(first, second, "同代同尺寸必须复用同一张画布");
        assert_eq!(cache.hits, 1, "第二次应当记一次命中");
    }

    #[test]
    fn newer_epoch_drops_buffers_held_by_this_thread() {
        let mut cache = SurfaceCache::new();
        cache.sync_epoch();
        cache.acquire_with_limit(4, 16, 16, 1024);
        assert_eq!(cache.slots.len(), 1, "取到画布后应当驻留一份");
        crate::memory::bump_surface_epoch();
        cache.sync_epoch();
        assert!(cache.slots.is_empty(), "换代之后必须立刻释放画布");
    }
}
