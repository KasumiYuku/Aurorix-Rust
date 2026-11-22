mod support;

use std::time::{Duration, Instant};

use aurorix_api::{
    ApiError, DedupWindow, Outbound, RateLimiter, Scene, SentBatch, Target, PUSH_RATE_LIMIT,
    PUSH_RATE_WINDOW, RECALL_GRACE, RECALL_WINDOW,
};
use support::MockServer;

#[test]
fn limiter_window_rejects_and_reopens() {
    let limiter = RateLimiter::without_rules();
    limiter.configure("push", 2, Duration::from_millis(50));
    assert!(limiter.check("push").is_ok(), "第 1 次放行");
    assert!(limiter.check("push").is_ok(), "第 2 次放行");
    let err = limiter.check("push").unwrap_err();
    match &err {
        ApiError::RateLimited { key, limit, .. } => {
            assert_eq!(key, "push");
            assert_eq!(*limit, 2);
        }
        other => panic!("限流要返回明确错误: {other}"),
    }
    std::thread::sleep(Duration::from_millis(70));
    assert!(limiter.check("push").is_ok(), "窗口过后重新放行");
}

#[test]
fn push_quota_is_twenty_per_minute() {
    let limiter = RateLimiter::new();
    for index in 0..PUSH_RATE_LIMIT {
        assert!(limiter.check("push").is_ok(), "第 {index} 次不该被限");
    }
    assert!(limiter.check("push").is_err(), "第 21 次必须报错");
    assert_eq!(PUSH_RATE_LIMIT, 20);
    assert_eq!(PUSH_RATE_WINDOW, Duration::from_secs(60));
}

#[test]
fn dedup_rejects_second_event_until_ttl() {
    let window = DedupWindow::new(Duration::from_millis(40));
    assert!(window.check_and_mark("e1"), "首次事件要放行");
    assert!(!window.check_and_mark("e1"), "第二次要判重");
    std::thread::sleep(Duration::from_millis(60));
    assert!(window.check_and_mark("e1"), "TTL 过后要重新放行");
    assert!(!window.check_and_mark("e1"), "重新放行之后又要判重");
}

#[test]
fn dedup_falls_back_to_message_id() {
    let window = DedupWindow::new(Duration::from_secs(30));
    assert!(window.check_and_mark_event("", "m1"));
    assert!(!window.check_and_mark_event("", "m1"));
    assert!(
        window.check_and_mark_event("e2", "m1"),
        "有 event_id 时优先用它"
    );
    assert!(!window.check_and_mark_event("e2", "m9"));
}

fn group_target() -> Target {
    Target::new(Scene::Group, "g1")
}

#[tokio::test]
async fn recall_inside_window_deletes_with_hidetip() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    api.set_recall_grace(Duration::ZERO);
    let batch = SentBatch::now(vec![String::from("m1"), String::new()]);
    let report = api.recall_batch(&group_target(), &[batch]).await;
    assert_eq!(report.recalled, vec![String::from("m1")]);
    assert!(report.failed.is_empty());
    let request = server.last();
    assert_eq!(request.method, "DELETE");
    assert_eq!(request.path, "/v2/groups/g1/messages/m1");
    assert_eq!(request.query.as_deref(), Some("hidetip=true"));
}

#[tokio::test]
async fn recall_outside_window_sends_nothing() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    api.set_recall_grace(Duration::ZERO);
    let old = Instant::now()
        .checked_sub(RECALL_WINDOW + Duration::from_secs(1))
        .expect("时间要能往回拨");
    let batch = SentBatch::new(vec![String::from("m1")], old);
    let report = api.recall_batch(&group_target(), &[batch]).await;
    assert_eq!(report.skipped, vec![String::from("m1")], "超窗直接跳过");
    assert!(report.recalled.is_empty());
    assert_eq!(server.api_count(), 0, "超窗不许发请求");
    assert_eq!(RECALL_WINDOW, Duration::from_secs(120));
}

#[tokio::test]
async fn active_delivery_is_not_recalled() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    api.set_recall_grace(Duration::ZERO);
    let batch = SentBatch::now(vec![String::from("m1")]).delivered_active();
    let report = api
        .recall_batch(&group_target(), std::slice::from_ref(&batch))
        .await;
    assert_eq!(report.skipped, vec![String::from("m1")]);
    assert_eq!(server.api_count(), 0, "主动投递的消息不撤回");
    assert!(!api.recallable(&batch));
}

#[tokio::test]
async fn recall_waits_for_the_grace_buffer() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    api.set_recall_grace(Duration::from_millis(60));
    assert_eq!(RECALL_GRACE, Duration::from_millis(400));
    let started = Instant::now();
    let batch = SentBatch::now(vec![String::from("m1")]);
    let report = api.recall_batch(&group_target(), &[batch]).await;
    let elapsed = started.elapsed();
    assert_eq!(report.recalled.len(), 1);
    assert!(
        elapsed >= Duration::from_millis(50),
        "发完要等缓冲再撤: {elapsed:?}"
    );
}

#[tokio::test]
async fn recall_reports_failures() {
    let server = MockServer::with_script(vec![support::MockResponse::platform(
        40064004,
        "已超出消息撤回时限",
    )])
    .await;
    let api = server.api();
    api.set_recall_grace(Duration::ZERO);
    let batch = SentBatch::now(vec![String::from("m1")]);
    let report = api.recall_batch(&group_target(), &[batch]).await;
    assert_eq!(report.recalled.len(), 0);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].id, "m1");
    assert!(report.failed[0].error.contains("撤回时限"));
    let _ = api
        .send(&group_target(), &Outbound::text("x"))
        .await
        .expect("发送仍要正常");
}
