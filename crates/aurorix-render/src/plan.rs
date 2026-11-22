use std::sync::OnceLock;
use std::time::Duration;

use aurorix_core::HardwareProfile;

use crate::pool::RenderPoolConfig;
use crate::scheduler::SchedulerConfig;

pub const MIN_WORKERS: usize = 2;
pub const MAX_WORKERS: usize = 8;
pub const QUEUE_PER_WORKER: usize = 64;
pub const WORKER_MEMORY_BUDGET: u64 = 64 * 1024 * 1024;
pub const MEMORY_DIVISOR: u64 = 24;
pub const MIN_MEMORY_BUDGET: u64 = 64 * 1024 * 1024;
pub const MAX_MEMORY_BUDGET: u64 = 512 * 1024 * 1024;
pub const DEFAULT_TASK_TIMEOUT_SECS: u64 = 30;
pub const FALLBACK_MEMORY_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererPlan {
    pub pool: RenderPoolConfig,
    pub scheduler: SchedulerConfig,
    pub surface_cache_bytes: u64,
}

impl RendererPlan {
    pub fn threads(&self) -> usize {
        self.pool.threads
    }

    pub fn queue_capacity(&self) -> usize {
        self.pool.queue_capacity
    }

    pub fn memory_budget(&self) -> u64 {
        self.scheduler.max_in_flight_bytes
    }
}

pub fn reserved_cpus(cpus: usize) -> usize {
    if cpus >= MAX_WORKERS {
        cpus / 4
    } else {
        1
    }
}

pub fn detect_workers(cpus: usize) -> usize {
    cpus.saturating_sub(reserved_cpus(cpus))
        .clamp(MIN_WORKERS, MAX_WORKERS)
}

pub fn detect_memory_budget(mem_bytes: u64) -> u64 {
    (mem_bytes / MEMORY_DIVISOR).clamp(MIN_MEMORY_BUDGET, MAX_MEMORY_BUDGET)
}

pub fn detect_affordable_workers(mem_bytes: u64) -> usize {
    let budget = detect_memory_budget(mem_bytes);
    let affordable = budget / WORKER_MEMORY_BUDGET;
    usize::try_from(affordable)
        .unwrap_or(MAX_WORKERS)
        .clamp(MIN_WORKERS, MAX_WORKERS)
}

pub fn detect_worker_count(cpus: usize, mem_bytes: u64) -> usize {
    detect_workers(cpus).min(detect_affordable_workers(mem_bytes))
}

pub fn detect_queue_capacity(workers: usize) -> usize {
    workers.max(1) * QUEUE_PER_WORKER
}

pub fn detect_pool_config(cpus: usize, mem_bytes: u64) -> RenderPoolConfig {
    let threads = detect_worker_count(cpus, mem_bytes);
    RenderPoolConfig {
        threads,
        queue_capacity: detect_queue_capacity(threads),
    }
}

pub fn detect_scheduler_config(cpus: usize, mem_bytes: u64) -> SchedulerConfig {
    let workers = detect_worker_count(cpus, mem_bytes);
    let budget = detect_memory_budget(mem_bytes);
    SchedulerConfig {
        workers,
        queue_capacity: detect_queue_capacity(workers),
        max_in_flight_bytes: budget,
        task_timeout: Duration::from_secs(DEFAULT_TASK_TIMEOUT_SECS),
    }
}

pub fn detect_plan(cpus: usize, mem_bytes: u64) -> RendererPlan {
    let budget = detect_memory_budget(mem_bytes);
    RendererPlan {
        pool: detect_pool_config(cpus, mem_bytes),
        scheduler: detect_scheduler_config(cpus, mem_bytes),
        surface_cache_bytes: budget,
    }
}

pub fn probe_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1)
}

pub fn probe_memory_bytes() -> u64 {
    let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") else {
        return FALLBACK_MEMORY_BYTES;
    };
    parse_meminfo_total(&meminfo).unwrap_or(FALLBACK_MEMORY_BYTES)
}

pub fn parse_meminfo_total(meminfo: &str) -> Option<u64> {
    for line in meminfo.lines() {
        let Some(rest) = line.trim().strip_prefix("MemTotal:") else {
            continue;
        };
        let kilobytes = rest
            .split_whitespace()
            .next()
            .and_then(|token| token.parse::<u64>().ok())?;
        return Some(kilobytes.saturating_mul(1024));
    }
    None
}

pub fn current_plan() -> RendererPlan {
    static PLAN: OnceLock<RendererPlan> = OnceLock::new();
    *PLAN.get_or_init(|| fit_to_hardware(detect_plan(probe_cpus(), probe_memory_bytes())))
}

pub fn surface_cache_budget() -> u64 {
    current_plan().surface_cache_bytes
}

fn fit_to_hardware(plan: RendererPlan) -> RendererPlan {
    let profile = HardwareProfile::cached();
    let threads = plan.pool.threads.min(profile.render_workers()).max(1);
    let budget = plan
        .scheduler
        .max_in_flight_bytes
        .min(profile.rss_soft_limit_bytes());
    RendererPlan {
        pool: RenderPoolConfig {
            threads,
            queue_capacity: detect_queue_capacity(threads),
        },
        scheduler: SchedulerConfig {
            workers: plan.scheduler.workers.min(threads).max(1),
            queue_capacity: detect_queue_capacity(threads),
            max_in_flight_bytes: budget,
            task_timeout: plan.scheduler.task_timeout,
        },
        surface_cache_bytes: if profile.surface_cache() == 0 {
            0
        } else {
            plan.surface_cache_bytes
        },
    }
}
