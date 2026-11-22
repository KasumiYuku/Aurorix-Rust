use std::sync::Arc;
use std::time::Duration;

use aurorix_bot::card_cache;
use aurorix_bot::ports::{Rendered, Renderer, SchedulerRenderer};
use aurorix_plugin::RenderCard;
use aurorix_render::skia_safe::Rect;
use aurorix_render::{Color, RenderRequest, RenderScheduler, SchedulerConfig, SkiaBackend};
use tokio::sync::{Mutex, MutexGuard};

const GUARD: Duration = Duration::from_secs(20);

static SERIAL: Mutex<()> = Mutex::const_new(());

async fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().await
}

fn scheduler_config() -> SchedulerConfig {
    SchedulerConfig {
        workers: 4,
        queue_capacity: 64,
        max_in_flight_bytes: 64 * 1024 * 1024,
        task_timeout: Duration::from_secs(15),
    }
}

fn card(key: Option<u64>, seed: f32) -> RenderCard {
    let request = RenderRequest::new(240, 160).with_background(Color::rgb(10, 12, 22));
    let drawn = RenderCard::new(request, "card.png", move |kit| {
        kit.fill_rect(
            Rect::from_xywh(0.0, 0.0, 240.0, 40.0),
            Color::rgb(24, 28, 48),
        );
        kit.fill_rect(
            Rect::from_xywh(20.0 + seed, 60.0, 120.0, 60.0),
            Color::rgb(32, 38, 62),
        );
    });
    match key {
        Some(key) => drawn.with_cache_key(key),
        None => drawn,
    }
}

fn bytes_of(rendered: Rendered) -> Vec<u8> {
    match rendered {
        Rendered::Image { bytes, .. } => bytes,
        Rendered::Text(text) => panic!("卡片渲染不应返回文本: {text}"),
    }
}

fn renderer() -> SchedulerRenderer {
    let scheduler = RenderScheduler::new(scheduler_config(), Arc::new(SkiaBackend::new()));
    SchedulerRenderer::new(scheduler)
}

#[tokio::test]
async fn case_011() {
    let guard = serial().await;
    tokio::time::timeout(GUARD, async {
        let renderer = renderer();
        card_cache::clear();
        let before = card_cache::renders();
        let first = bytes_of(
            renderer
                .render_card(&card(Some(0xA1), 0.0))
                .await
                .expect("首次渲染应当成功"),
        );
        let second = bytes_of(
            renderer
                .render_card(&card(Some(0xA1), 0.0))
                .await
                .expect("第二次应当命中缓存"),
        );
        let third = bytes_of(
            renderer
                .render_card(&card(Some(0xA1), 0.0))
                .await
                .expect("第三次应当命中缓存"),
        );
        assert_eq!(
            card_cache::renders() - before,
            1,
            "同一指纹只允许真正渲染一次"
        );
        assert_eq!(first, second, "缓存命中必须与首次结果逐字节相同");
        assert_eq!(first, third, "缓存命中必须与首次结果逐字节相同");
        assert!(
            card_cache::used_bytes() >= first.len(),
            "命中后的字节应当计入缓存占用"
        );
        assert_eq!(card_cache::entries(), 1, "同一指纹只应有一个缓存条目");
        let plain = bytes_of(
            renderer
                .render_card(&card(None, 0.0))
                .await
                .expect("未缓存路径应当成功"),
        );
        assert_eq!(first, plain, "经过缓存的字节必须与直接渲染逐字节相同");
    })
    .await
    .expect("同一内容重复查询未在限时内完成");
    drop(guard);
}

#[tokio::test]
async fn case_012() {
    let guard = serial().await;
    tokio::time::timeout(GUARD, async {
        let renderer = Arc::new(renderer());
        card_cache::clear();
        let before = card_cache::renders();
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let renderer = Arc::clone(&renderer);
            tasks.push(tokio::spawn(async move {
                renderer
                    .render_card(&card(Some(0xB2), 0.0))
                    .await
                    .map(bytes_of)
                    .expect("并发渲染应当成功")
            }));
        }
        let mut outputs = Vec::new();
        for task in tasks {
            outputs.push(task.await.expect("渲染任务不应崩溃"));
        }
        assert_eq!(
            card_cache::renders() - before,
            1,
            "并发同指纹必须合并为一次渲染"
        );
        for output in &outputs {
            assert_eq!(output, &outputs[0], "并发共享的结果必须逐字节相同");
        }
        assert_eq!(card_cache::entries(), 1, "并发合并后只应留下一个条目");
    })
    .await
    .expect("并发同指纹请求未在限时内完成");
    drop(guard);
}

#[tokio::test]
async fn case_013() {
    let guard = serial().await;
    tokio::time::timeout(GUARD, async {
        let renderer = renderer();
        card_cache::clear();
        let before = card_cache::renders();
        let left = bytes_of(
            renderer
                .render_card(&card(Some(0xC3), 0.0))
                .await
                .expect("左侧渲染应当成功"),
        );
        let right = bytes_of(
            renderer
                .render_card(&card(Some(0xC4), 40.0))
                .await
                .expect("右侧渲染应当成功"),
        );
        let again = bytes_of(
            renderer
                .render_card(&card(Some(0xC4), 40.0))
                .await
                .expect("右侧复查询应当命中"),
        );
        assert_eq!(card_cache::renders() - before, 2, "两个指纹各渲染一次");
        assert_ne!(left, right, "不同画面不应得到相同字节");
        assert_eq!(right, again, "命中的字节必须与首次结果逐字节相同");
        assert_eq!(card_cache::entries(), 2, "两个指纹应各有一个条目");
    })
    .await
    .expect("不同指纹渲染未在限时内完成");
    drop(guard);
}

#[test]
fn case_014() {
    let guard = SERIAL.blocking_lock();
    card_cache::clear();
    let tier = aurorix_core::HardwareProfile::cached().tier();
    let expected = match tier {
        aurorix_core::HardwareTier::Nano => 8 << 20,
        aurorix_core::HardwareTier::Compact => 24 << 20,
        aurorix_core::HardwareTier::Balanced => 64 << 20,
        aurorix_core::HardwareTier::Performance => 128 << 20,
    };
    assert_eq!(
        card_cache::budget_bytes(),
        expected,
        "整卡缓存预算必须跟随硬件档位"
    );
    assert_eq!(card_cache::used_bytes(), 0, "清空后占用必须归零");
    assert_eq!(card_cache::entries(), 0, "清空后条目必须归零");
    drop(guard);
}
