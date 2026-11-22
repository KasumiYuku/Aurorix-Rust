use std::future::poll_fn;
use std::pin::Pin;
use std::time::Duration;

use aurorix_admin::{router, AdminState, TOPIC_LOG};
use aurorix_db::AppConfig;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::json;
use tokio_stream::Stream;
use tower::ServiceExt;

fn state() -> AdminState {
    AdminState::new("config.json", AppConfig::default())
}

#[tokio::test]
async fn unknown_admin_api_path_returns_json_404() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/admin/api/does-not-exist")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("路由应可调用");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let kind = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        kind.starts_with("application/json"),
        "未命中的 /admin/api/* 必须回 JSON, 实际为 {kind}"
    );
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 16)
        .await
        .expect("读响应体");
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("响应体需为 JSON");
    assert!(body.get("error").is_some(), "JSON 404 必须带 error 字段");
}

#[tokio::test]
async fn unknown_spa_path_still_serves_index() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/admin/some/deep/route")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("路由应可调用");
    assert_eq!(response.status(), StatusCode::OK, "非接口路径必须交给 SPA");
    let kind = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(kind.starts_with("text/html"), "SPA 兜底必须回 HTML");
}

#[tokio::test]
async fn stream_replays_events_after_last_event_id() {
    let state = state();
    let bus = state.live.clone();
    bus.publish(TOPIC_LOG, json!({ "msg": "one" }));
    bus.publish(TOPIC_LOG, json!({ "msg": "two" }));
    bus.publish(TOPIC_LOG, json!({ "msg": "three" }));
    let (token, _) = state.sessions.issue_for(Duration::from_secs(60));
    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/admin/api/stream")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header("last-event-id", "1")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("路由应可调用");
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let chunk = tokio::time::timeout(
        Duration::from_secs(5),
        poll_fn(|context| Pin::new(&mut body).poll_next(context)),
    )
    .await
    .expect("回放必须在超时之前到达")
    .expect("回放不该结束")
    .expect("回放不该出错");
    let text = String::from_utf8_lossy(&chunk).to_string();
    assert!(
        text.contains("id: 2"),
        "必须从 Last-Event-ID 之后回放, 实际为 {text}"
    );
    assert!(
        !text.contains("id: 1"),
        "断线之前的事件不该重复推送, 实际为 {text}"
    );
}
