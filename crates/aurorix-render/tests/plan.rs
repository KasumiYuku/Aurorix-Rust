use std::time::Duration;

use aurorix_render::plan::{
    detect_affordable_workers, detect_memory_budget, detect_plan, detect_pool_config,
    detect_queue_capacity, detect_scheduler_config, detect_worker_count, detect_workers,
    parse_meminfo_total, reserved_cpus, MAX_MEMORY_BUDGET, MAX_WORKERS, MIN_MEMORY_BUDGET,
    MIN_WORKERS, QUEUE_PER_WORKER,
};

const MIB: u64 = 1024 * 1024;

#[test]
fn single_and_small_cpu_counts_keep_headroom() {
    assert_eq!(reserved_cpus(1), 1, "低核数时固定预留 1 个核");
    assert_eq!(reserved_cpus(7), 1, "低于 8 核时固定预留 1 个核");
    assert_eq!(detect_workers(0), MIN_WORKERS, "0 核也必须给出可用线程数");
    assert_eq!(detect_workers(1), 2, "单核应当补足到 2 个 worker");
    assert_eq!(detect_workers(2), 2, "双核应当得到 2 个 worker");
    assert_eq!(detect_workers(4), 3, "4 核应当预留 1 核得到 3 个 worker");
    assert_eq!(detect_workers(5), 4, "5 核应当预留 1 核得到 4 个 worker");
}

#[test]
fn many_cores_are_capped_and_reserve_a_quarter() {
    assert_eq!(reserved_cpus(8), 2, "8 核应当预留 2 个核");
    assert_eq!(reserved_cpus(16), 4, "16 核应当预留 4 个核");
    assert_eq!(detect_workers(8), 6, "8 核应当得到 6 个 worker");
    assert_eq!(detect_workers(12), MAX_WORKERS, "12 核应当被夹到上限");
    assert_eq!(detect_workers(16), 8, "16 核应当被夹到 8 个 worker");
    assert_eq!(detect_workers(128), MAX_WORKERS, "超高核数应当被夹到上限");
}

#[test]
fn memory_budget_respects_floor_and_ceiling() {
    assert_eq!(
        detect_memory_budget(0),
        MIN_MEMORY_BUDGET,
        "极小内存应当落到下限"
    );
    assert_eq!(
        detect_memory_budget(MIB),
        MIN_MEMORY_BUDGET,
        "1MiB 内存应当落到 64MiB 下限"
    );
    assert_eq!(
        detect_memory_budget(24 * 200 * MIB),
        200 * MIB,
        "内存的 1/24 应当被用作预算"
    );
    assert_eq!(
        detect_memory_budget(24 * 512 * MIB),
        MAX_MEMORY_BUDGET,
        "刚好达到上限时应当等于上限"
    );
    assert_eq!(
        detect_memory_budget(u64::MAX),
        MAX_MEMORY_BUDGET,
        "超大内存应当被夹到 512MiB 上限"
    );
}

#[test]
fn affordable_workers_follow_the_memory_budget() {
    assert_eq!(
        detect_affordable_workers(0),
        2,
        "64MiB 下限预算只够 1 个 worker, 补足到下限 2"
    );
    assert_eq!(
        detect_affordable_workers(24 * 200 * MIB),
        3,
        "200MiB 预算按 64MiB 单 worker 预算只能给出 3 个 worker"
    );
}

#[test]
fn pool_config_scales_queue_with_workers() {
    let config = detect_pool_config(4, 24 * 200 * MIB);
    assert_eq!(config.threads, 3, "4 核 200MiB 应当得到 3 个线程");
    assert_eq!(
        config.queue_capacity,
        3 * QUEUE_PER_WORKER,
        "队列容量应当随 worker 数线性增长"
    );
    assert_eq!(
        detect_queue_capacity(5),
        5 * QUEUE_PER_WORKER,
        "队列容量应当是 worker 数的 64 倍"
    );
}

#[test]
fn pool_config_is_limited_by_tiny_memory() {
    let config = detect_pool_config(16, 1);
    assert_eq!(config.threads, 2, "极小内存应当把 16 核限制到 2 个线程");
    assert_eq!(
        config.queue_capacity,
        2 * QUEUE_PER_WORKER,
        "受限后队列容量应当跟随实际线程数"
    );
    let huge = detect_pool_config(16, u64::MAX);
    assert_eq!(huge.threads, MAX_WORKERS, "内存充裕时 16 核应当用满上限");
    assert_eq!(
        huge.queue_capacity,
        MAX_WORKERS * QUEUE_PER_WORKER,
        "内存充裕时队列容量应当取上限线程数的 64 倍"
    );
}

#[test]
fn scheduler_config_matches_the_detector() {
    let config = detect_scheduler_config(16, u64::MAX);
    assert_eq!(config.workers, 8, "16 核大内存应当用满 8 个 worker");
    assert_eq!(
        config.queue_capacity,
        8 * QUEUE_PER_WORKER,
        "队列应当按 worker 数放大"
    );
    assert_eq!(
        config.max_in_flight_bytes, MAX_MEMORY_BUDGET,
        "字节预算应当取 512MiB 上限"
    );
    assert_eq!(
        config.task_timeout,
        Duration::from_secs(30),
        "默认超时应当保持 30 秒"
    );
    let tiny = detect_scheduler_config(1, 1);
    assert_eq!(
        tiny.workers, MIN_WORKERS,
        "单核极小内存应当得到 2 个 worker"
    );
    assert_eq!(
        tiny.max_in_flight_bytes, MIN_MEMORY_BUDGET,
        "极小内存应当落到 64MiB 下限"
    );
    assert_eq!(
        tiny.queue_capacity,
        MIN_WORKERS * QUEUE_PER_WORKER,
        "单核队列容量应当为 2 * 64"
    );
}

#[test]
fn worker_count_takes_the_minimum_of_cpu_and_memory() {
    assert_eq!(
        detect_worker_count(16, u64::MAX),
        MAX_WORKERS,
        "取两者较小值"
    );
    assert_eq!(
        detect_worker_count(1, u64::MAX),
        MIN_WORKERS,
        "CPU 更少时取 CPU"
    );
    assert_eq!(
        detect_worker_count(16, 1),
        2,
        "内存更少时取内存允许的 worker 数"
    );
}

#[test]
fn plan_combines_both_configs_and_cache_budget() {
    let plan = detect_plan(8, 24 * 300 * MIB);
    assert_eq!(plan.threads(), 4, "300MiB 预算按 64MiB 只够 4 个线程");
    assert_eq!(plan.queue_capacity(), 4 * QUEUE_PER_WORKER, "队列应当一致");
    assert_eq!(
        plan.memory_budget(),
        300 * MIB,
        "内存预算应当等于内存的 1/24"
    );
    assert_eq!(
        plan.surface_cache_bytes,
        300 * MIB,
        "表面缓存上限应当与内存预算一致"
    );
    assert_eq!(
        plan.pool,
        detect_pool_config(8, 24 * 300 * MIB),
        "池配置应当一致"
    );
    assert_eq!(
        plan.scheduler,
        detect_scheduler_config(8, 24 * 300 * MIB),
        "调度配置应当一致"
    );
}

#[test]
fn meminfo_total_is_parsed_with_fallback() {
    let sample = "MemTotal:       16384000 kB\nMemFree:         1234 kB\n";
    assert_eq!(
        parse_meminfo_total(sample),
        Some(16384000 * 1024),
        "应当把 kB 换算成字节"
    );
    assert_eq!(
        parse_meminfo_total("MemFree: 10 kB\nMemTotal: 2048 kB\n"),
        Some(2048 * 1024),
        "应当跳过 MemTotal 之前的行"
    );
    assert_eq!(
        parse_meminfo_total("MemFree: 10 kB\n"),
        None,
        "缺少字段应当返回 None"
    );
    assert_eq!(parse_meminfo_total(""), None, "空内容应当返回 None");
    assert_eq!(
        parse_meminfo_total("MemTotal: abc kB\n"),
        None,
        "非法数字应当返回 None"
    );
}
