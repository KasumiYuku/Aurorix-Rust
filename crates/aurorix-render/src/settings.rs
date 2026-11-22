use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::draw::RenderFormat;

pub const MIN_RENDER_BUDGET_MB: u64 = 8;

pub const MAX_RENDER_BUDGET_MB: u64 = 4096;

pub const MIN_RENDER_CONCURRENCY: usize = 1;

pub const MAX_RENDER_CONCURRENCY: usize = 64;

pub const MIN_RENDER_QUEUE: usize = 1;

pub const MAX_RENDER_QUEUE: usize = 4096;

static WEBP_ENABLED: AtomicBool = AtomicBool::new(true);

static RENDER_BUDGET_MB: AtomicU64 = AtomicU64::new(0);

static RENDER_CONCURRENCY: AtomicU64 = AtomicU64::new(0);

static RENDER_QUEUE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSettings {
    pub webp_enabled: bool,
    pub render_budget_mb: u64,
    pub render_concurrency: usize,
    pub render_queue: usize,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            webp_enabled: true,
            render_budget_mb: 0,
            render_concurrency: 0,
            render_queue: 0,
        }
    }
}

impl RenderSettings {
    pub fn sanitized(self) -> Self {
        Self {
            webp_enabled: self.webp_enabled,
            render_budget_mb: clamp_budget(self.render_budget_mb),
            render_concurrency: clamp_concurrency(self.render_concurrency),
            render_queue: clamp_queue(self.render_queue),
        }
    }
}

fn clamp_budget(megabytes: u64) -> u64 {
    if megabytes == 0 {
        0
    } else {
        megabytes.clamp(MIN_RENDER_BUDGET_MB, MAX_RENDER_BUDGET_MB)
    }
}

fn clamp_concurrency(workers: usize) -> usize {
    if workers == 0 {
        0
    } else {
        workers.clamp(MIN_RENDER_CONCURRENCY, MAX_RENDER_CONCURRENCY)
    }
}

fn clamp_queue(depth: usize) -> usize {
    if depth == 0 {
        0
    } else {
        depth.clamp(MIN_RENDER_QUEUE, MAX_RENDER_QUEUE)
    }
}

pub fn current_settings() -> RenderSettings {
    RenderSettings {
        webp_enabled: webp_enabled(),
        render_budget_mb: render_budget_mb(),
        render_concurrency: render_concurrency(),
        render_queue: render_queue(),
    }
}

pub fn apply_settings(settings: RenderSettings) {
    let settings = settings.sanitized();
    WEBP_ENABLED.store(settings.webp_enabled, Ordering::SeqCst);
    RENDER_BUDGET_MB.store(settings.render_budget_mb, Ordering::SeqCst);
    RENDER_CONCURRENCY.store(settings.render_concurrency as u64, Ordering::SeqCst);
    RENDER_QUEUE.store(settings.render_queue as u64, Ordering::SeqCst);
}

pub fn webp_enabled() -> bool {
    WEBP_ENABLED.load(Ordering::SeqCst)
}

pub fn set_webp_enabled(enabled: bool) {
    WEBP_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn render_budget_mb() -> u64 {
    RENDER_BUDGET_MB.load(Ordering::SeqCst)
}

pub fn set_render_budget_mb(megabytes: u64) {
    RENDER_BUDGET_MB.store(clamp_budget(megabytes), Ordering::SeqCst);
}

pub fn render_budget_bytes() -> Option<u64> {
    match render_budget_mb() {
        0 => None,
        megabytes => Some(megabytes << 20),
    }
}

pub fn render_concurrency() -> usize {
    RENDER_CONCURRENCY.load(Ordering::SeqCst) as usize
}

pub fn set_render_concurrency(workers: usize) {
    RENDER_CONCURRENCY.store(clamp_concurrency(workers) as u64, Ordering::SeqCst);
}

pub fn effective_concurrency(default_workers: usize) -> usize {
    match render_concurrency() {
        0 => default_workers.max(1),
        workers => workers,
    }
}

pub fn render_queue() -> usize {
    RENDER_QUEUE.load(Ordering::SeqCst) as usize
}

pub fn set_render_queue(depth: usize) {
    RENDER_QUEUE.store(clamp_queue(depth) as u64, Ordering::SeqCst);
}

pub fn effective_queue(default_depth: usize) -> usize {
    match render_queue() {
        0 => default_depth.max(1),
        depth => depth,
    }
}

pub fn effective_format(format: RenderFormat) -> RenderFormat {
    if format == RenderFormat::WebP && !webp_enabled() {
        RenderFormat::Png
    } else {
        format
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_takes_effect_immediately() {
        apply_settings(RenderSettings::default());
        assert!(webp_enabled(), "默认应当开启 webp");
        assert_eq!(render_budget_bytes(), None, "默认预算应当交给硬件档位");
        assert_eq!(effective_concurrency(3), 3, "默认并发应当沿用配置值");
        assert_eq!(effective_queue(64), 64, "默认队列应当沿用配置值");
        assert_eq!(effective_format(RenderFormat::WebP), RenderFormat::WebP);

        set_webp_enabled(false);
        assert!(!webp_enabled());
        assert_eq!(
            effective_format(RenderFormat::WebP),
            RenderFormat::Png,
            "关掉 webp 之后请求 webp 也必须出 png"
        );
        assert_eq!(
            effective_format(RenderFormat::Jpeg),
            RenderFormat::Jpeg,
            "关掉 webp 不该影响其它格式"
        );

        set_render_budget_mb(96);
        assert_eq!(render_budget_mb(), 96);
        assert_eq!(render_budget_bytes(), Some(96 << 20));
        assert_eq!(
            crate::card_cache::budget_bytes(),
            96usize << 20,
            "运行时预算必须立刻作用到卡片缓存"
        );
        assert_eq!(current_settings().render_budget_mb, 96);
        set_render_budget_mb(0);
        assert_eq!(render_budget_bytes(), None);

        set_render_concurrency(2);
        assert_eq!(render_concurrency(), 2);
        assert_eq!(effective_concurrency(8), 2, "运行时并发应当覆盖默认值");

        set_render_queue(32);
        assert_eq!(render_queue(), 32);
        assert_eq!(effective_queue(512), 32, "运行时队列深度应当覆盖默认值");

        apply_settings(RenderSettings {
            webp_enabled: true,
            render_budget_mb: 128,
            render_concurrency: 4,
            render_queue: 128,
        });
        assert_eq!(
            current_settings(),
            RenderSettings {
                webp_enabled: true,
                render_budget_mb: 128,
                render_concurrency: 4,
                render_queue: 128,
            }
        );
        apply_settings(RenderSettings::default());
        assert_eq!(current_settings(), RenderSettings::default());
    }

    #[test]
    fn settings_are_clamped_to_sane_bounds() {
        let sanitized = RenderSettings {
            webp_enabled: false,
            render_budget_mb: 1,
            render_concurrency: 10_000,
            render_queue: 10_000,
        }
        .sanitized();
        assert_eq!(sanitized.render_budget_mb, MIN_RENDER_BUDGET_MB);
        assert_eq!(sanitized.render_concurrency, MAX_RENDER_CONCURRENCY);
        assert_eq!(sanitized.render_queue, MAX_RENDER_QUEUE);
        assert_eq!(
            RenderSettings {
                render_budget_mb: 0,
                render_concurrency: 0,
                render_queue: 0,
                ..RenderSettings::default()
            }
            .sanitized()
            .render_budget_mb,
            0
        );
    }
}
