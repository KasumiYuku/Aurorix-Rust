use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aurorix_core::{HardwareProfile, HardwareTier};
use aurorix_log::Logger;
use tokio::sync::Semaphore;

use crate::bot::MODULE;

pub const POOL_MAX: usize = 256;

pub const POOL_QUEUE: usize = 1024;

pub const POOL_TIMEOUT: Duration = Duration::from_secs(30);

pub fn limits() -> (usize, usize) {
    limits_for(HardwareProfile::cached().tier())
}

pub fn limits_for(tier: HardwareTier) -> (usize, usize) {
    match tier {
        HardwareTier::Nano => (8, 32),
        HardwareTier::Compact => (32, 128),
        HardwareTier::Balanced => (128, 512),
        HardwareTier::Performance => (POOL_MAX, POOL_QUEUE),
    }
}

pub struct WorkPool {
    max: usize,
    queue: usize,
    timeout: Duration,
    running: Arc<Semaphore>,
    pending: Arc<Semaphore>,
    timeouts: AtomicU64,
    logger: Arc<Logger>,
}

impl std::fmt::Debug for WorkPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkPool")
            .field("max", &self.max)
            .field("queue", &self.queue)
            .field("running", &self.running())
            .field("in_flight", &self.in_flight())
            .finish()
    }
}

impl WorkPool {
    pub fn new(max: usize, queue: usize, timeout: Duration, logger: Arc<Logger>) -> Arc<Self> {
        let max = max.max(1);
        let queue = queue.max(max);
        Arc::new(Self {
            max,
            queue,
            timeout,
            running: Arc::new(Semaphore::new(max)),
            pending: Arc::new(Semaphore::new(queue)),
            timeouts: AtomicU64::new(0),
            logger,
        })
    }

    pub fn from_hardware(logger: Arc<Logger>) -> Arc<Self> {
        let (max, queue) = limits();
        Self::new(max, queue, POOL_TIMEOUT, logger)
    }

    pub fn max(&self) -> usize {
        self.max
    }

    pub fn queue(&self) -> usize {
        self.queue
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn running(&self) -> usize {
        self.max - self.running.available_permits()
    }

    pub fn in_flight(&self) -> usize {
        self.queue - self.pending.available_permits()
    }

    pub fn timeouts(&self) -> u64 {
        self.timeouts.load(Ordering::SeqCst)
    }

    pub async fn dispatch<F>(self: &Arc<Self>, label: &'static str, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let slot = Arc::clone(&self.pending)
            .acquire_owned()
            .await
            .expect("事件池队列信号量不会被关闭");
        let pool = Arc::clone(self);
        tokio::spawn(async move {
            let _slot = slot;
            let permit = Arc::clone(&pool.running)
                .acquire_owned()
                .await
                .expect("事件池并发信号量不会被关闭");
            if tokio::time::timeout(pool.timeout, task).await.is_err() {
                pool.timeouts.fetch_add(1, Ordering::SeqCst);
                pool.logger.warn(
                    MODULE,
                    &format!(
                        "{label}超过 {} 秒未完成, 已放弃该任务",
                        pool.timeout.as_secs()
                    ),
                );
            }
            drop(permit);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurorix_log::Options;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::Barrier;

    fn logger() -> Arc<Logger> {
        Logger::new(Options::default())
    }

    #[test]
    fn hardware_tiers_never_exceed_the_default_caps() {
        for tier in [
            HardwareTier::Nano,
            HardwareTier::Compact,
            HardwareTier::Balanced,
            HardwareTier::Performance,
        ] {
            let (max, queue) = limits_for(tier);
            assert!(max <= POOL_MAX, "并发上限不得超过默认上限");
            assert!(queue <= POOL_QUEUE, "队列上限不得超过默认上限");
            assert!(queue >= max, "队列必须能装下所有并发任务");
        }
    }

    #[tokio::test]
    async fn two_tasks_run_concurrently() {
        let pool = WorkPool::new(2, 8, Duration::from_secs(5), logger());
        let barrier = Arc::new(Barrier::new(2));
        let peak = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let peak = Arc::clone(&peak);
            let done = Arc::clone(&done);
            pool.dispatch("并发测试", async move {
                let now = peak.fetch_add(1, Ordering::SeqCst) + 1;
                let _ = tokio::time::timeout(Duration::from_secs(2), barrier.wait()).await;
                peak.fetch_max(now, Ordering::SeqCst);
                done.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        }
        for _ in 0..400 {
            if done.load(Ordering::SeqCst) == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(done.load(Ordering::SeqCst), 2, "两个任务都要跑完");
        assert_eq!(peak.load(Ordering::SeqCst), 2, "两个任务必须同时在跑");
    }

    #[tokio::test]
    async fn timeout_releases_the_permit() {
        let pool = WorkPool::new(1, 4, Duration::from_millis(20), logger());
        pool.dispatch("超时测试", async {
            tokio::time::sleep(Duration::from_secs(30)).await;
        })
        .await;
        for _ in 0..400 {
            if pool.timeouts() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(pool.timeouts(), 1, "超时任务必须被记下来");
        for _ in 0..400 {
            if pool.running() == 0 && pool.in_flight() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(pool.running(), 0, "超时后并发许可必须还回去");
        assert_eq!(pool.in_flight(), 0, "超时后队列许可必须还回去");
    }

    #[tokio::test]
    async fn a_full_queue_applies_backpressure_instead_of_dropping() {
        let pool = WorkPool::new(1, 1, Duration::from_secs(5), logger());
        let gate = Arc::new(Semaphore::new(0));
        let first_gate = Arc::clone(&gate);
        pool.dispatch("占位", async move {
            let _ = first_gate.acquire().await;
        })
        .await;
        for _ in 0..400 {
            if pool.in_flight() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(pool.in_flight(), 1, "第一个任务要占住唯一的槽位");
        let entered = Arc::new(AtomicUsize::new(0));
        let second = Arc::clone(&entered);
        let queued = Arc::clone(&pool);
        let blocked = tokio::spawn(async move {
            queued
                .dispatch("排队", async move {
                    second.fetch_add(1, Ordering::SeqCst);
                })
                .await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !blocked.is_finished(),
            "队列满时派发必须阻塞, 而不是丢任务"
        );
        assert_eq!(entered.load(Ordering::SeqCst), 0, "阻塞期间不该执行");
        gate.add_permits(1);
        blocked.await.expect("排队任务要能收尾");
        for _ in 0..400 {
            if entered.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(entered.load(Ordering::SeqCst), 1, "槽位空出来后排队任务要跑");
    }
}
