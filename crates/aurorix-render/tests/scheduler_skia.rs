use std::sync::Arc;
use std::time::Duration;

use aurorix_render::{
    fill_rect, skia_safe, Color, RenderContext, RenderError, RenderFormat, RenderRequest,
    RenderScheduler, RenderTask, SchedulerConfig, SkiaBackend, TextStyle,
};
use skia_safe::{Data, Image};

const GUARD: Duration = Duration::from_secs(60);

fn config(task_timeout: Duration) -> SchedulerConfig {
    SchedulerConfig {
        workers: 4,
        queue_capacity: 64,
        max_in_flight_bytes: 64 * 1024 * 1024,
        task_timeout,
    }
}

fn scheduler(task_timeout: Duration) -> Arc<RenderScheduler> {
    RenderScheduler::new(config(task_timeout), Arc::new(SkiaBackend::new()))
}

fn tile_request() -> RenderRequest {
    RenderRequest::new(200, 80)
        .with_background(Color::WHITE)
        .with_quality(85)
}

fn png_dimensions(bytes: &[u8]) -> (i32, i32) {
    let image = Image::from_encoded(Data::new_copy(bytes)).expect("输出应当可以解码");
    (image.width(), image.height())
}

#[tokio::test]
async fn submit_render_completes_every_task() {
    tokio::time::timeout(GUARD, async {
        let scheduler = scheduler(Duration::from_secs(30));
        let style = TextStyle::new("sans-serif", 28);
        let mut handles = Vec::new();
        for index in 0..8i32 {
            let request = tile_request();
            let task = RenderTask::new(format!("tile-{index}"))
                .with_estimated_bytes(request.estimated_bytes());
            let style = style.clone();
            let handle = scheduler
                .submit_render(task, request, move |canvas, context| {
                    let offset = index as f32 * 4.0;
                    fill_rect(canvas, offset, offset, 60.0, 24.0, Color::rgb(20, 60, 160));
                    context
                        .draw_text(canvas, "Aurorix", 8.0, 64.0, &style)
                        .expect("任务内文字绘制应当成功");
                })
                .await
                .expect("绘制任务应当提交成功");
            handles.push(handle);
        }
        for handle in handles {
            let ticket = handle.ticket();
            let output = handle.wait().await.expect("绘制任务应当完成");
            assert_eq!(output.ticket, ticket, "输出票据应当与提交票据一致");
            assert!(
                output.bytes.starts_with(b"\xff\xd8"),
                "任务输出应当是合法 JPEG"
            );
            assert_eq!(
                png_dimensions(&output.bytes),
                (200, 80),
                "每个任务的画布尺寸应当独立且一致"
            );
        }
        let metrics = scheduler.metrics();
        assert_eq!(metrics.completed, 8, "八个绘制任务都应当完成");
        assert_eq!(metrics.failed, 0, "不应存在失败任务");
        assert_eq!(metrics.in_flight, 0, "不应残留在飞任务");
        scheduler.shutdown().await;
    })
    .await
    .expect("并发绘制测试超时");
}

#[tokio::test]
async fn panicking_render_is_reported_as_panicked() {
    tokio::time::timeout(GUARD, async {
        let scheduler = scheduler(Duration::from_secs(30));
        let handle = scheduler
            .submit_render(
                RenderTask::new("panic").with_estimated_bytes(4096),
                RenderRequest::new(64, 48),
                |_canvas, _context| {
                    panic!("绘制任务故意崩溃");
                },
            )
            .await
            .expect("崩溃任务应当提交成功");
        let outcome = handle.wait().await;
        assert!(
            matches!(outcome, Err(RenderError::Panicked)),
            "绘制任务 panic 应当被捕获为 Panicked"
        );
        assert_eq!(scheduler.metrics().failed, 1, "失败计数应当为 1");
        scheduler.shutdown().await;
    })
    .await
    .expect("panic 捕获测试超时");
}

#[tokio::test]
async fn slow_render_times_out() {
    tokio::time::timeout(GUARD, async {
        let scheduler = scheduler(Duration::from_millis(1));
        let handle = scheduler
            .submit_render(
                RenderTask::new("slow").with_estimated_bytes(4096),
                RenderRequest::new(200, 80),
                |canvas, _context| {
                    for step in 0..50_000 {
                        let offset = (step % 32) as f32;
                        fill_rect(canvas, offset, offset, 4.0, 4.0, Color::rgb(30, 30, 30));
                    }
                },
            )
            .await
            .expect("慢任务应当提交成功");
        let outcome = handle.wait().await;
        assert!(
            matches!(outcome, Err(RenderError::Timeout)),
            "超出超时的绘制任务应当返回 Timeout"
        );
        assert_eq!(scheduler.metrics().completed, 0, "超时任务不计入完成");
        assert_eq!(scheduler.metrics().failed, 1, "超时任务计入失败");
        scheduler.shutdown().await;
    })
    .await
    .expect("超时绘制测试超时");
}

#[tokio::test]
async fn render_submission_respects_byte_budget() {
    tokio::time::timeout(GUARD, async {
        let scheduler = RenderScheduler::new(
            SchedulerConfig {
                workers: 2,
                queue_capacity: 8,
                max_in_flight_bytes: 1024,
                task_timeout: Duration::from_secs(5),
            },
            Arc::new(SkiaBackend::new()),
        );
        let oversized = scheduler
            .submit_render(
                RenderTask::new("oversized").with_estimated_bytes(8192),
                RenderRequest::new(200, 80),
                |_canvas, _context| {},
            )
            .await;
        assert!(
            matches!(oversized, Err(RenderError::BytesBudgetExceeded)),
            "超出字节预算的绘制任务应当被拒绝"
        );
        let accepted = scheduler
            .submit_render(
                RenderTask::new("fits").with_estimated_bytes(512),
                RenderRequest::new(32, 32).with_format(RenderFormat::Png),
                |canvas, _context| {
                    fill_rect(canvas, 0.0, 0.0, 16.0, 16.0, Color::BLACK);
                },
            )
            .await
            .expect("预算内的绘制任务应当提交成功");
        let output = accepted.wait().await.expect("预算内任务应当完成");
        assert_eq!(
            png_dimensions(&output.bytes),
            (32, 32),
            "预算内任务应当产出合法画布"
        );
        assert_eq!(scheduler.metrics().rejected, 1, "被拒绝的任务应当计数一次");
        scheduler.shutdown().await;
    })
    .await
    .expect("字节预算测试超时");
}

#[tokio::test]
async fn concurrent_draws_do_not_share_canvas() {
    tokio::time::timeout(GUARD, async {
        let context: Arc<RenderContext> =
            RenderContext::new(Arc::new(SkiaBackend::new()), 8, 1 << 20);
        let workers = (0..8i32)
            .map(|index| {
                let context = Arc::clone(&context);
                std::thread::spawn(move || {
                    let request = RenderRequest::new(40 + index, 20 + index);
                    let bytes = context
                        .draw(request, |canvas, _context| {
                            fill_rect(canvas, 0.0, 0.0, 8.0, 8.0, Color::rgb(9, 9, 9));
                        })
                        .expect("并发绘制应当成功");
                    png_dimensions(&bytes)
                })
            })
            .collect::<Vec<_>>();
        for (index, worker) in workers.into_iter().enumerate() {
            let expected = (40 + index as i32, 20 + index as i32);
            assert_eq!(
                worker.join().expect("绘制线程不应崩溃"),
                expected,
                "每个任务应当拿到自己尺寸的画布"
            );
        }
    })
    .await
    .expect("并发画布隔离测试超时");
}
