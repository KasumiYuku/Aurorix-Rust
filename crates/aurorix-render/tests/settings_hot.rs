use std::sync::Arc;
use std::time::Duration;

use aurorix_render::{
    fill_rect, render_concurrency, render_queue, set_render_concurrency, set_render_queue,
    set_webp_enabled, webp_enabled, BackendHandle, Color, PlaceholderBackend, RenderContext,
    RenderFormat, RenderRequest, RenderScheduler, RenderTask, SchedulerConfig, SkiaBackend,
};
use bytes::Bytes;

const GUARD: Duration = Duration::from_secs(60);

#[tokio::test]
async fn disabling_webp_switches_renders_to_png() {
    tokio::time::timeout(GUARD, async {
        let context = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 8 * 1024 * 1024);
        set_webp_enabled(false);
        let request = RenderRequest::new(32, 24).with_format(RenderFormat::WebP);
        let image = context
            .draw_image(request, |canvas, _context| {
                fill_rect(canvas, 2.0, 2.0, 8.0, 8.0, Color::BLACK);
            })
            .expect("关掉 webp 之后仍然必须出图");
        set_webp_enabled(true);
        assert!(webp_enabled());
        assert_eq!(image.format, RenderFormat::Png, "关掉 webp 必须改出 png");
        assert_eq!(image.mime(), "image/png");
        assert!(
            image.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "回退产物必须是 png 字节"
        );
    })
    .await
    .expect("webp 开关测试超时");
}

#[tokio::test]
async fn scheduler_applies_runtime_concurrency_and_queue() {
    tokio::time::timeout(GUARD, async {
        let backend: BackendHandle = Arc::new(PlaceholderBackend::new());
        let config = SchedulerConfig {
            workers: 2,
            queue_capacity: 16,
            max_in_flight_bytes: 8 * 1024 * 1024,
            task_timeout: Duration::from_secs(10),
        };
        let scheduler = RenderScheduler::try_new(config, backend).expect("调度器应当可以创建");
        set_render_concurrency(3);
        set_render_queue(9);
        assert_eq!(render_concurrency(), 3);
        assert_eq!(render_queue(), 9);
        let handle = scheduler
            .submit(RenderTask::new("hot"), Box::new(|_context| {
                Ok(Bytes::from_static(b"hot"))
            }))
            .await
            .expect("提交应当成功");
        let output = handle.wait().await.expect("任务应当完成");
        assert_eq!(output.bytes, Bytes::from_static(b"hot"));
        let metrics = scheduler.metrics();
        assert_eq!(metrics.workers, 3, "运行时并发必须作用到 worker 目标数");
        assert_eq!(metrics.queued, 0);
        assert_eq!(metrics.reserved_bytes, 0, "任务结束后字节预算必须归还");
        set_render_concurrency(0);
        set_render_queue(0);
        assert_eq!(render_concurrency(), 0);
        assert_eq!(render_queue(), 0);
        scheduler.shutdown().await;
    })
    .await
    .expect("运行时并发测试超时");
}

#[tokio::test]
async fn timeout_keeps_the_byte_budget_until_the_job_finishes() {
    tokio::time::timeout(GUARD, async {
        let backend: BackendHandle = Arc::new(PlaceholderBackend::new());
        let config = SchedulerConfig {
            workers: 1,
            queue_capacity: 8,
            max_in_flight_bytes: 8 * 1024 * 1024,
            task_timeout: Duration::from_millis(20),
        };
        let scheduler = RenderScheduler::try_new(config, backend).expect("调度器应当可以创建");
        let handle = scheduler
            .submit(
                RenderTask::new("slow").with_estimated_bytes(1024),
                Box::new(|_context| {
                    std::thread::sleep(Duration::from_millis(150));
                    Ok(Bytes::from_static(b"slow"))
                }),
            )
            .await
            .expect("提交应当成功");
        let outcome = handle.wait().await;
        assert!(outcome.is_err(), "超时的任务必须报错");
        assert!(
            scheduler.metrics().reserved_bytes > 0,
            "任务还在跑的时候不许提前归还字节预算"
        );
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(
            scheduler.metrics().reserved_bytes,
            0,
            "任务真正结束之后必须归还字节预算"
        );
        scheduler.shutdown().await;
    })
    .await
    .expect("超时预算测试超时");
}
