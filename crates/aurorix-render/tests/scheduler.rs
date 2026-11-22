use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_render::{
    BackendHandle, PlaceholderBackend, RenderError, RenderHandle, RenderJob, RenderPriority,
    RenderScheduler, RenderTask, SchedulerConfig,
};
use bytes::Bytes;

const GUARD: Duration = Duration::from_secs(10);

fn placeholder() -> BackendHandle {
    Arc::new(PlaceholderBackend::new())
}

fn config(
    workers: usize,
    queue_capacity: usize,
    max_in_flight_bytes: u64,
    task_timeout: Duration,
) -> SchedulerConfig {
    SchedulerConfig {
        workers,
        queue_capacity,
        max_in_flight_bytes,
        task_timeout,
    }
}

fn quick_config(workers: usize, queue_capacity: usize) -> SchedulerConfig {
    config(
        workers,
        queue_capacity,
        4 * 1024 * 1024,
        Duration::from_secs(5),
    )
}

fn sized_job(size: usize) -> RenderJob {
    Box::new(move |_context| Ok(Bytes::from(vec![9u8; size])))
}

fn blocking_job(
    started: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
) -> RenderJob {
    Box::new(move |_context| {
        let _ = started.send(());
        let _ = release.recv();
        Ok(Bytes::from_static(b"blocked"))
    })
}

async fn wait_started(started: std::sync::mpsc::Receiver<()>) {
    tokio::time::timeout(GUARD, tokio::task::spawn_blocking(move || started.recv()))
        .await
        .expect("等待占位任务启动超时")
        .expect("等待线程意外退出")
        .expect("占位任务应当已启动");
}

#[tokio::test]
async fn single_task_produces_output() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(quick_config(2, 8), placeholder());
        let task = RenderTask::new("preview")
            .with_priority(RenderPriority::High)
            .with_estimated_bytes(512);
        let handle = scheduler
            .submit(task, sized_job(1024))
            .await
            .expect("提交应当成功");
        let ticket = handle.ticket();
        let output = handle.wait().await.expect("任务应当完成");
        assert_eq!(output.ticket, ticket, "票据编号应当保持一致");
        assert_eq!(output.bytes.len(), 1024, "输出字节数应当与任务一致");
        let metrics = scheduler.metrics();
        assert_eq!(metrics.submitted, 1, "提交计数应当为 1");
        assert_eq!(metrics.completed, 1, "完成计数应当为 1");
        assert_eq!(metrics.failed, 0, "失败计数应当为 0");
        assert_eq!(metrics.rejected, 0, "拒绝计数应当为 0");
        assert_eq!(metrics.in_flight, 0, "不应存在在飞任务");
        assert_eq!(metrics.queued, 0, "队列应当已排空");
        scheduler.shutdown().await;
    })
    .await
    .expect("单任务测试超时");
}

#[tokio::test]
async fn high_priority_runs_before_low() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(quick_config(1, 8), placeholder());
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let blocker = scheduler
            .submit(
                RenderTask::new("blocker"),
                blocking_job(started_tx, release_rx),
            )
            .await
            .expect("占位任务应当提交成功");
        wait_started(started_rx).await;

        let low = {
            let order = Arc::clone(&order);
            scheduler
                .submit(
                    RenderTask::new("low").with_priority(RenderPriority::Low),
                    Box::new(move |_context| {
                        order.lock().expect("记录锁应当可用").push("low");
                        Ok(Bytes::from_static(b"low"))
                    }),
                )
                .await
                .expect("低优先级任务应当提交成功")
        };
        let high = {
            let order = Arc::clone(&order);
            scheduler
                .submit(
                    RenderTask::new("high").with_priority(RenderPriority::High),
                    Box::new(move |_context| {
                        order.lock().expect("记录锁应当可用").push("high");
                        Ok(Bytes::from_static(b"high"))
                    }),
                )
                .await
                .expect("高优先级任务应当提交成功")
        };

        release_tx.send(()).expect("释放占位任务失败");
        high.wait().await.expect("高优先级任务应当完成");
        low.wait().await.expect("低优先级任务应当完成");
        blocker.wait().await.expect("占位任务应当完成");

        let recorded = {
            let guard = order.lock().expect("记录锁应当可用");
            guard.clone()
        };
        assert_eq!(recorded, vec!["high", "low"], "高优先级任务应当先完成");
        scheduler.shutdown().await;
    })
    .await
    .expect("优先级测试超时");
}

#[tokio::test]
async fn queue_full_is_rejected() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(quick_config(1, 1), placeholder());
        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let blocker = scheduler
            .submit(
                RenderTask::new("blocker"),
                blocking_job(started_tx, release_rx),
            )
            .await
            .expect("占位任务应当提交成功");
        wait_started(started_rx).await;

        let queued = scheduler
            .submit(RenderTask::new("queued"), sized_job(16))
            .await
            .expect("队列应当还能容纳一个任务");
        let overflow = scheduler
            .submit(RenderTask::new("overflow"), sized_job(16))
            .await;
        assert!(
            matches!(overflow, Err(RenderError::QueueFull)),
            "队列已满时应当拒绝新任务"
        );
        let metrics = scheduler.metrics();
        assert_eq!(metrics.rejected, 1, "拒绝计数应当为 1");
        assert_eq!(metrics.queued, 1, "队列中应当只有一个任务");

        release_tx.send(()).expect("释放占位任务失败");
        queued.wait().await.expect("排队任务应当完成");
        blocker.wait().await.expect("占位任务应当完成");
        scheduler.shutdown().await;
    })
    .await
    .expect("队列满测试超时");
}

#[tokio::test]
async fn bytes_budget_is_enforced() {
    tokio::time::timeout(GUARD, async {
        let scheduler =
            RenderScheduler::new(config(1, 8, 1000, Duration::from_secs(5)), placeholder());
        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let blocker = scheduler
            .submit(
                RenderTask::new("blocker").with_estimated_bytes(600),
                blocking_job(started_tx, release_rx),
            )
            .await
            .expect("占位任务应当提交成功");
        wait_started(started_rx).await;

        let oversized = scheduler
            .submit(
                RenderTask::new("oversized").with_estimated_bytes(1500),
                sized_job(8),
            )
            .await;
        assert!(
            matches!(oversized, Err(RenderError::BytesBudgetExceeded)),
            "单任务超出预算应当被拒绝"
        );

        let exceeding = scheduler
            .submit(
                RenderTask::new("exceeding").with_estimated_bytes(500),
                sized_job(8),
            )
            .await;
        assert!(
            matches!(exceeding, Err(RenderError::BytesBudgetExceeded)),
            "累计超出预算应当被拒绝"
        );

        let fitting = scheduler
            .submit(
                RenderTask::new("fitting").with_estimated_bytes(400),
                sized_job(8),
            )
            .await
            .expect("预算内的任务应当提交成功");

        release_tx.send(()).expect("释放占位任务失败");
        fitting.wait().await.expect("预算内任务应当完成");
        blocker.wait().await.expect("占位任务应当完成");
        assert_eq!(scheduler.metrics().rejected, 2, "拒绝计数应当为 2");
        scheduler.shutdown().await;
    })
    .await
    .expect("字节预算测试超时");
}

#[tokio::test]
async fn slow_job_times_out() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(
            config(2, 8, 4 * 1024 * 1024, Duration::from_millis(50)),
            placeholder(),
        );
        let handle = scheduler
            .submit(
                RenderTask::new("slow"),
                Box::new(|_context| {
                    std::thread::sleep(Duration::from_millis(400));
                    Ok(Bytes::from_static(b"late"))
                }),
            )
            .await
            .expect("超时任务应当提交成功");
        let outcome = handle.wait().await;
        assert!(
            matches!(outcome, Err(RenderError::Timeout)),
            "超时任务应当返回 Timeout"
        );
        let metrics = scheduler.metrics();
        assert_eq!(metrics.failed, 1, "失败计数应当为 1");
        assert_eq!(metrics.completed, 0, "完成计数应当为 0");
        scheduler.shutdown().await;
    })
    .await
    .expect("超时测试超时");
}

#[tokio::test]
async fn panicking_job_is_captured() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(quick_config(2, 8), placeholder());
        let handle = scheduler
            .submit(
                RenderTask::new("panic"),
                Box::new(|_context| -> Result<Bytes, RenderError> {
                    panic!("渲染任务故意崩溃")
                }),
            )
            .await
            .expect("崩溃任务应当提交成功");
        let outcome = handle.wait().await;
        assert!(
            matches!(outcome, Err(RenderError::Panicked)),
            "任务 panic 应当被捕获为 Panicked"
        );
        assert_eq!(scheduler.metrics().failed, 1, "失败计数应当为 1");
        scheduler.shutdown().await;
    })
    .await
    .expect("panic 捕获测试超时");
}

#[tokio::test]
async fn job_error_is_passed_through() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(quick_config(2, 8), placeholder());
        let handle = scheduler
            .submit(
                RenderTask::new("failing"),
                Box::new(|_context| Err(RenderError::Backend(String::from("后端渲染失败")))),
            )
            .await
            .expect("失败任务应当提交成功");
        match handle.wait().await {
            Err(RenderError::Backend(message)) => {
                assert_eq!(message, "后端渲染失败", "错误消息应当原样透传");
            }
            other => panic!("应当透传后端错误, 实际为 {other:?}"),
        }
        assert_eq!(scheduler.metrics().failed, 1, "失败计数应当为 1");
        scheduler.shutdown().await;
    })
    .await
    .expect("错误透传测试超时");
}

#[tokio::test]
async fn shutdown_rejects_new_and_drains_in_flight() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(quick_config(2, 8), placeholder());
        let pending: RenderHandle = scheduler
            .submit(
                RenderTask::new("drain"),
                Box::new(|_context| {
                    std::thread::sleep(Duration::from_millis(120));
                    Ok(Bytes::from_static(b"drained"))
                }),
            )
            .await
            .expect("关闭前任务应当提交成功");

        scheduler.shutdown().await;
        assert!(scheduler.is_closed(), "调度器应当处于关闭状态");

        let late = scheduler
            .submit(RenderTask::new("late"), sized_job(4))
            .await;
        assert!(
            matches!(late, Err(RenderError::Shutdown)),
            "关闭后提交应当返回 Shutdown"
        );

        let output = pending.wait().await.expect("关闭前排队的任务应当完成");
        assert_eq!(output.bytes.len(), 7, "在飞任务应当产出结果");

        scheduler.shutdown().await;
    })
    .await
    .expect("关闭测试超时");
}

#[tokio::test]
async fn metrics_track_lifecycle() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(quick_config(2, 8), placeholder());
        let good = scheduler
            .submit(RenderTask::new("good"), sized_job(32))
            .await
            .expect("成功任务应当提交成功");
        let bad = scheduler
            .submit(
                RenderTask::new("bad"),
                Box::new(|_context| Err(RenderError::Font(String::from("字体缺失")))),
            )
            .await
            .expect("失败任务应当提交成功");
        good.wait().await.expect("成功任务应当完成");
        bad.wait().await.expect_err("失败任务应当返回错误");

        let metrics = scheduler.metrics();
        assert_eq!(metrics.submitted, 2, "提交计数应当为 2");
        assert_eq!(metrics.completed, 1, "完成计数应当为 1");
        assert_eq!(metrics.failed, 1, "失败计数应当为 1");
        assert_eq!(metrics.rejected, 0, "拒绝计数应当为 0");
        assert_eq!(metrics.in_flight, 0, "不应存在在飞任务");
        assert_eq!(metrics.queued, 0, "队列应当已排空");
        scheduler.shutdown().await;
    })
    .await
    .expect("指标测试超时");
}

#[tokio::test]
async fn tickets_start_at_one_and_step_by_one() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(quick_config(2, 8), placeholder());
        let mut handles = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..3 {
            let handle = scheduler
                .submit(RenderTask::new("ticket"), sized_job(8))
                .await
                .expect("提交应当成功");
            ids.push(handle.ticket());
            handles.push(handle);
        }
        assert_eq!(ids, vec![1, 2, 3], "票据编号应当从 1 开始逐个加 1");
        for handle in handles {
            let ticket = handle.ticket();
            let output = handle.wait().await.expect("任务应当完成");
            assert_eq!(output.ticket, ticket, "输出票据应当与提交票据一致");
        }
        assert_eq!(scheduler.metrics().submitted, 3, "提交计数应当为 3");
        scheduler.shutdown().await;
    })
    .await
    .expect("票据测试超时");
}

#[tokio::test]
async fn shutdown_waits_for_queue_and_in_flight_before_returning() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(quick_config(2, 8), placeholder());
        let pending = scheduler
            .submit(
                RenderTask::new("drain"),
                Box::new(|_context| {
                    std::thread::sleep(Duration::from_millis(200));
                    Ok(Bytes::from_static(b"drained"))
                }),
            )
            .await
            .expect("关闭前任务应当提交成功");

        scheduler.shutdown().await;
        assert!(scheduler.is_closed(), "调度器应当处于关闭状态");
        let metrics = scheduler.metrics();
        assert_eq!(metrics.in_flight, 0, "shutdown 返回时不该还有在飞任务");
        assert_eq!(metrics.queued, 0, "shutdown 返回时队列应当已排空");
        assert_eq!(metrics.completed, 1, "shutdown 应当等到任务真正完成");

        let late = scheduler
            .submit(RenderTask::new("late"), sized_job(4))
            .await;
        assert!(
            matches!(late, Err(RenderError::Shutdown)),
            "关闭后提交应当返回 Shutdown"
        );
        let output = pending.wait().await.expect("在飞任务应当拿到结果");
        assert_eq!(output.bytes.len(), 7, "在飞任务应当产出结果");
    })
    .await
    .expect("关闭排空测试超时");
}
