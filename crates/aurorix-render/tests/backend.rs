use std::sync::Arc;
use std::time::Duration;

use aurorix_render::{
    PlaceholderBackend, RenderBackend, RenderScheduler, SchedulerConfig, SkiaBackend,
};

const GUARD: Duration = Duration::from_secs(10);

#[tokio::test]
async fn placeholder_backend_reports_cpu_capabilities() {
    tokio::time::timeout(GUARD, async {
        let backend = PlaceholderBackend::new();
        assert_eq!(backend.name(), "placeholder-cpu", "占位后端名称应当固定");
        let capabilities = backend.capabilities();
        assert!(
            !capabilities.hardware_accelerated,
            "占位后端不应启用硬件加速"
        );
        assert!(
            capabilities.max_texture_side >= 1024,
            "纹理边长上限应当为正数"
        );
        assert!(capabilities.max_texture_bytes > 0, "纹理字节上限应当为正数");
        assert!(
            capabilities.concurrent_contexts >= 1,
            "并发上下文数应当不小于 1"
        );
    })
    .await
    .expect("占位后端测试超时");
}

#[tokio::test]
async fn skia_backend_reports_real_capabilities() {
    tokio::time::timeout(GUARD, async {
        let backend = SkiaBackend::new();
        assert_eq!(backend.name(), "skia-safe", "skia 后端名称应当固定");
        let capabilities = backend.capabilities();
        assert_eq!(
            capabilities.hardware_accelerated,
            aurorix_render::gpu::hardware_accelerated(),
            "skia 后端的硬件加速标记应当与 GPU 探测结果一致"
        );
        assert!(
            backend.font_family_count() > 0,
            "系统字体管理器应当能枚举字体族"
        );
        assert!(
            capabilities.concurrent_contexts >= 1,
            "并发上下文数应当不小于 1"
        );
        assert!(
            capabilities.max_texture_side >= 4096,
            "skia 纹理边长上限应当足够大"
        );
        assert!(
            capabilities.max_texture_bytes >= 64 * 1024 * 1024,
            "skia 纹理字节上限应当足够大"
        );
    })
    .await
    .expect("skia 后端测试超时");
}

#[tokio::test]
async fn scheduler_defaults_are_applied() {
    tokio::time::timeout(GUARD, async {
        let config = SchedulerConfig::default();
        assert!(
            (2..=8).contains(&config.workers),
            "默认 worker 数应当落在 2..=8, 实际为 {}",
            config.workers
        );
        assert_eq!(
            config.queue_capacity,
            config.workers * 64,
            "默认队列容量应当随 worker 数线性放大"
        );
        assert!(
            (64 * 1024 * 1024..=512 * 1024 * 1024).contains(&config.max_in_flight_bytes),
            "默认字节预算应当落在 64MiB..=512MiB, 实际为 {}",
            config.max_in_flight_bytes
        );
        assert_eq!(
            config.task_timeout,
            Duration::from_secs(30),
            "默认超时应当为 30 秒"
        );

        let scheduler = RenderScheduler::new(config, Arc::new(SkiaBackend::new()));
        assert_eq!(
            scheduler.backend().name(),
            "skia-safe",
            "调度器应当持有注入的后端"
        );
        assert_eq!(
            scheduler.context().backend().name(),
            "skia-safe",
            "渲染上下文应当复用同一后端"
        );
        assert_eq!(
            scheduler.context().textures().capacity_bytes(),
            config.max_in_flight_bytes / 2,
            "纹理缓存容量应当取字节预算的一半"
        );
        assert_eq!(
            scheduler.context().fonts().capacity(),
            64,
            "字体簿默认容量应当为 64"
        );
        assert!(!scheduler.is_closed(), "新建调度器不应处于关闭状态");
        scheduler.shutdown().await;
    })
    .await
    .expect("默认配置测试超时");
}
