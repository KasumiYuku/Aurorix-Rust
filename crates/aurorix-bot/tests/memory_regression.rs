use std::time::Duration;

use aurorix_admin::SessionStore;
use aurorix_api::DedupWindow;
use aurorix_core::{resident_bytes, trim_memory, HardwareProfile};
use aurorix_log::{Logger, Options};
use aurorix_plugin::{Rate, RateKey, RateLimiter};
use aurorix_render::memory;

const ROUNDS: usize = 2000;

const RENDERS: u64 = 10;

const DEDUP_CAPACITY: usize = 256;

const RATE_CAPACITY: usize = 64;

const LOG_CAPACITY: usize = 256;

const LOG_BYTES: usize = 1 << 20;

const RSS_BUDGET: u64 = 32 << 20;

const WINDOW: Duration = Duration::from_secs(600);

#[test]
fn memory_stays_bounded_across_message_rounds() {
    let profile = HardwareProfile::cached();
    let baseline = resident_bytes();
    let dedup = DedupWindow::with_capacity(WINDOW, DEDUP_CAPACITY);
    let limiter = RateLimiter::with_capacity(RATE_CAPACITY);
    let sessions = SessionStore::with_ttl_seconds(3600);
    let logger = Logger::new(
        Options::default()
            .with_capacity(LOG_CAPACITY)
            .with_byte_budget(LOG_BYTES),
    );
    let rate = Rate::per_window(5, 60);
    for round in 0..ROUNDS {
        assert!(
            dedup.check_and_mark(&format!("event-{round}")),
            "第 {round} 轮的事件不该被判成重复"
        );
        let key = RateKey::new("sign", "group", format!("user-{round}"));
        assert!(
            limiter.check(key, rate).is_ok(),
            "第 {round} 轮的新用户不该被限流"
        );
        let _ = sessions.issue();
        logger.info("memory_test", &format!("第 {round} 轮消息处理完成"));
    }
    assert!(
        dedup.len() <= DEDUP_CAPACITY,
        "去重窗口超过上限, 实际 {} 条",
        dedup.len()
    );
    assert!(
        limiter.len() <= RATE_CAPACITY,
        "限流桶超过上限, 实际 {} 个",
        limiter.len()
    );
    assert!(
        sessions.len() <= sessions.capacity(),
        "登录会话超过上限, 实际 {} 个",
        sessions.len()
    );
    assert!(
        logger.ring().len() <= LOG_CAPACITY,
        "日志内存环超过上限, 实际 {} 条",
        logger.ring().len()
    );
    assert!(
        profile.name_cache_capacity() > 0 && profile.dedup_capacity() > 0,
        "硬件档位给出的缓存上限必须为正"
    );
    for _ in 0..RENDERS {
        memory::after_render();
    }
    assert!(
        memory::renders() >= RENDERS,
        "渲染计数没有累计, 实际 {}",
        memory::renders()
    );
    assert!(
        memory::font_cache_limit() > 0 && memory::resource_cache_limit() > 0,
        "Skia 缓存上限必须为正"
    );
    assert!(memory::surface_cache_limit() > 0, "表面池上限必须为正");
    let _ = trim_memory();
    logger.flush();
    drop(logger);
    drop(sessions);
    drop(limiter);
    drop(dedup);
    trim_memory();
    let after = resident_bytes();
    assert!(
        after.saturating_sub(baseline) < RSS_BUDGET,
        "2000 轮消息后常驻内存涨了 {} 字节, 超过预算",
        after.saturating_sub(baseline)
    );
}
