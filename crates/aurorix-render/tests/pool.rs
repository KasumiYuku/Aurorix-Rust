use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use aurorix_render::{
    pool, Color, PlaceholderBackend, RenderContext, RenderError, RenderPool, RenderPoolConfig,
    RenderPriority, RenderRequest, SkiaBackend,
};
use bytes::Bytes;

fn blocking_job(
    started: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
) -> impl FnOnce() -> Result<Bytes, RenderError> + Send + 'static {
    move || {
        let _ = started.send(());
        let _ = release.recv();
        Ok(Bytes::from_static(b"blocked"))
    }
}

fn context() -> Arc<RenderContext> {
    RenderContext::new(Arc::new(SkiaBackend::new()), 8, 8 * 1024 * 1024)
}

fn draw(context: &RenderContext, width: i32, height: i32) -> usize {
    let request = RenderRequest::new(width, height).with_background(Color::WHITE);
    context
        .draw(request, |canvas, _context| {
            aurorix_render::fill_rect(canvas, 1.0, 1.0, 8.0, 8.0, Color::BLACK);
        })
        .expect("绘制应当成功")
        .len()
}

#[test]
fn twenty_concurrent_jobs_complete() {
    let pool = RenderPool::with_config(
        RenderPoolConfig::default()
            .with_threads(4)
            .with_queue_capacity(64),
    )
    .expect("线程池应当创建成功");
    let counter = Arc::new(AtomicUsize::new(0));
    let mut tickets = Vec::new();
    for index in 0..20usize {
        let counter = Arc::clone(&counter);
        tickets.push(
            pool.submit(
                RenderPriority::Normal,
                Box::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(Bytes::from(vec![index as u8; index + 1]))
                }),
            )
            .expect("任务应当提交成功"),
        );
    }
    let mut lengths = Vec::new();
    for ticket in tickets {
        let bytes = ticket.wait_blocking().expect("任务应当成功完成");
        lengths.push(bytes.len());
    }
    lengths.sort_unstable();
    let expected: Vec<usize> = (1..=20).collect();
    assert_eq!(lengths, expected, "20 个并发任务的结果应当完整且正确");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        20,
        "每个任务都应当被执行一次"
    );
    let stats = pool.stats();
    assert_eq!(stats.completed, 20, "完成计数应当为 20");
    assert_eq!(stats.failed, 0, "不应存在失败任务");
    pool.shutdown();
}

#[test]
fn queue_full_is_rejected_with_pool_error() {
    let pool = RenderPool::with_config(
        RenderPoolConfig::default()
            .with_threads(1)
            .with_queue_capacity(1),
    )
    .expect("线程池应当创建成功");
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let running = pool
        .submit(
            RenderPriority::Normal,
            Box::new(blocking_job(started_tx, release_rx)),
        )
        .expect("占位任务应当提交成功");
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("占位任务应当已经开始执行");

    let queued = pool
        .submit(
            RenderPriority::Normal,
            Box::new(|| Ok(Bytes::from_static(b"queued"))),
        )
        .expect("队列应当还能容纳一个任务");
    let overflow = pool.submit(
        RenderPriority::Normal,
        Box::new(|| Ok(Bytes::from_static(b"overflow"))),
    );
    match overflow {
        Err(RenderError::PoolQueueFull { capacity }) => {
            assert_eq!(capacity, 1, "错误应当带上队列容量");
        }
        other => panic!("队列已满时应当返回 PoolQueueFull, 实际为 {other:?}"),
    }
    assert_eq!(pool.stats().rejected, 1, "拒绝计数应当为 1");

    release_tx.send(()).expect("释放占位任务失败");
    running.wait_blocking().expect("占位任务应当完成");
    queued.wait_blocking().expect("排队任务应当完成");
    pool.shutdown();
}

#[test]
fn shutdown_terminates_all_threads() {
    let pool = RenderPool::with_config(RenderPoolConfig::default().with_threads(3))
        .expect("线程池应当创建成功");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while pool.live_threads() < 3 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(pool.live_threads(), 3, "三个工作线程都应当已启动");
    pool.shutdown();
    assert_eq!(pool.live_threads(), 0, "关闭后不应残留工作线程");
    assert!(pool.is_closed(), "关闭后线程池应当标记为已关闭");
    let late = pool.submit(RenderPriority::Normal, Box::new(|| Ok(Bytes::new())));
    assert!(
        matches!(late, Err(RenderError::Shutdown)),
        "关闭后提交应当返回 Shutdown"
    );
}

#[test]
fn panicking_job_does_not_kill_the_pool() {
    let pool = RenderPool::with_config(RenderPoolConfig::default().with_threads(2))
        .expect("线程池应当创建成功");
    let outcome = pool.execute(RenderPriority::High, || panic!("工作线程内故意崩溃"));
    assert!(
        matches!(outcome, Err(RenderError::Panicked)),
        "panic 应当被捕获为 Panicked"
    );
    let healthy = pool
        .execute(RenderPriority::Normal, || Ok(Bytes::from_static(b"alive")))
        .expect("崩溃后线程池仍应可用");
    assert_eq!(healthy.len(), 5, "崩溃后任务仍应正常返回结果");
    assert_eq!(pool.stats().failed, 1, "失败计数应当为 1");
    assert_eq!(pool.stats().completed, 1, "完成计数应当为 1");
    pool.shutdown();
    assert_eq!(pool.live_threads(), 0, "关闭后线程应当全部退出");
}

#[test]
fn priority_orders_queued_jobs() {
    let pool = RenderPool::with_config(
        RenderPoolConfig::default()
            .with_threads(1)
            .with_queue_capacity(8),
    )
    .expect("线程池应当创建成功");
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let blocker = pool
        .submit(
            RenderPriority::Normal,
            Box::new(blocking_job(started_tx, release_rx)),
        )
        .expect("占位任务应当提交成功");
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("占位任务应当已经开始执行");

    let order = Arc::new(parking_lot::Mutex::new(Vec::<&'static str>::new()));
    let mut tickets = Vec::new();
    for (name, priority) in [
        ("low", RenderPriority::Low),
        ("high", RenderPriority::High),
        ("normal", RenderPriority::Normal),
    ] {
        let order = Arc::clone(&order);
        tickets.push(
            pool.submit(
                priority,
                Box::new(move || {
                    order.lock().push(name);
                    Ok(Bytes::new())
                }),
            )
            .expect("任务应当提交成功"),
        );
    }
    release_tx.send(()).expect("释放占位任务失败");
    blocker.wait_blocking().expect("占位任务应当完成");
    for ticket in tickets {
        ticket.wait_blocking().expect("排队任务应当完成");
    }
    let recorded = order.lock().clone();
    assert_eq!(
        recorded,
        vec!["high", "normal", "low"],
        "线程池应当按优先级出队"
    );
    pool.shutdown();
}

#[test]
fn same_size_reuses_the_pixel_buffer() {
    let context = context();
    let before = pool::thread_reuse_stats();
    draw(&context, 200, 100);
    let after_first = pool::thread_reuse_stats();
    assert_eq!(after_first.hits, before.hits, "首次渲染同尺寸应当未命中");
    draw(&context, 200, 100);
    let after_second = pool::thread_reuse_stats();
    assert!(
        after_second.hits > after_first.hits,
        "第二次同尺寸渲染应当命中复用: {} -> {}",
        after_first.hits,
        after_second.hits
    );
    assert_eq!(
        after_second.allocations, after_first.allocations,
        "复用时不应再次分配像素缓冲"
    );
    assert!(after_second.hit_rate() > 0.0, "复用命中率应当大于 0");
}

#[test]
fn different_size_does_not_reuse() {
    let context = context();
    draw(&context, 180, 90);
    let before = pool::thread_reuse_stats();
    draw(&context, 120, 60);
    let after = pool::thread_reuse_stats();
    assert_eq!(after.hits, before.hits, "不同尺寸不应计入复用命中");
    assert!(after.misses > before.misses, "不同尺寸应当计入未命中");
}

#[test]
fn draw_through_pool_threads_keeps_results_correct() {
    let context = context();
    let pool = RenderPool::with_config(RenderPoolConfig::default().with_threads(3))
        .expect("线程池应当创建成功");
    let mut tickets = Vec::new();
    for index in 0..12i32 {
        let context = Arc::clone(&context);
        tickets.push(
            pool.submit(
                RenderPriority::Normal,
                Box::new(move || {
                    let request =
                        RenderRequest::new(64 + index, 48).with_background(Color::rgb(10, 20, 30));
                    context.draw(request, |canvas, _context| {
                        aurorix_render::fill_rect(canvas, 2.0, 2.0, 16.0, 16.0, Color::WHITE);
                    })
                }),
            )
            .expect("任务应当提交成功"),
        );
    }
    for ticket in tickets {
        let bytes = ticket.wait_blocking().expect("池内渲染应当成功");
        assert!(
            bytes.starts_with(b"\xff\xd8"),
            "池内渲染应当产出合法 JPEG"
        );
    }
    assert_eq!(pool.stats().completed, 12, "12 个池内渲染都应当完成");
    pool.shutdown();
}

#[test]
fn placeholder_backend_context_renders_on_pool() {
    let backend = Arc::new(PlaceholderBackend::new()) as aurorix_render::BackendHandle;
    let context = RenderContext::new(backend, 4, 1024 * 1024);
    let pool = RenderPool::with_config(RenderPoolConfig::default().with_threads(2))
        .expect("线程池应当创建成功");
    let (width, height) = (40, 24);
    let context_inner = Arc::clone(&context);
    let bytes = pool
        .execute(RenderPriority::High, move || {
            context_inner.draw(RenderRequest::new(width, height), |_canvas, _context| {})
        })
        .expect("占位后端也应当可以渲染");
    assert!(!bytes.is_empty(), "输出不应为空");
    pool.shutdown();
}

#[test]
fn shutdown_wakes_every_idle_worker() {
    for _ in 0..40 {
        let pool = RenderPool::with_config(RenderPoolConfig::default().with_threads(2))
            .expect("线程池应当创建成功");
        let (done_tx, done_rx) = mpsc::channel();
        {
            let pool = Arc::clone(&pool);
            std::thread::spawn(move || {
                pool.shutdown();
                let _ = done_tx.send(());
            });
        }
        let finished = done_rx.recv_timeout(Duration::from_secs(5)).is_ok();
        assert!(
            finished,
            "关闭线程池时必须唤醒每一个空闲工作线程, 不允许丢失唤醒"
        );
        assert_eq!(pool.live_threads(), 0, "关闭后不应残留工作线程");
    }
}
