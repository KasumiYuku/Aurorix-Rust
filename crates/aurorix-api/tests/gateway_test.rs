mod support;

use std::time::Duration;

use support::{MockResponse, MockServer};

const GATEWAY_BODY: &str = r#"{"url":"wss://api.sgroup.qq.com/websocket","shards":4,
"session_start_limit":{"total":1000,"remaining":0,"reset_after":120000,"max_concurrency":1}}"#;

#[tokio::test]
async fn gateway_bot_reads_url_shards_and_quota() {
    let server = MockServer::with_script(vec![MockResponse::json(GATEWAY_BODY)]).await;
    let api = server.api();
    let info = api.gateway_bot().await.expect("要能取到网关信息");
    assert_eq!(server.paths(), vec![String::from("/gateway/bot")]);
    assert_eq!(info.url, "wss://api.sgroup.qq.com/websocket");
    assert_eq!(info.shard_count(), 4);
    assert_eq!(info.shard(), [0, 4]);
    assert!(info.exhausted(), "remaining 为 0 就是配额耗尽");
    assert_eq!(info.reset_after(), Duration::from_millis(120_000));
}

#[tokio::test]
async fn gateway_bot_keeps_the_quota_when_sessions_remain() {
    let server = MockServer::with_script(vec![MockResponse::json(
        r#"{"url":"wss://api.sgroup.qq.com/websocket","shards":1,
        "session_start_limit":{"total":1000,"remaining":12,"reset_after":60000}}"#,
    )])
    .await;
    let info = server.api().gateway_bot().await.expect("要能取到网关信息");
    assert_eq!(info.shard_count(), 1);
    assert_eq!(info.shard(), [0, 1]);
    assert!(!info.exhausted(), "还有余额就不算耗尽");
}

#[tokio::test]
async fn gateway_bot_rejects_an_empty_url() {
    let server = MockServer::with_script(vec![MockResponse::json(
        r#"{"url":"   ","shards":1,"session_start_limit":{"remaining":5}}"#,
    )])
    .await;
    let error = server.api().gateway_bot().await.expect_err("空地址要报错");
    assert!(error.to_string().contains("网关地址为空"), "{error}");
}

#[tokio::test]
async fn gateway_bot_rejects_an_empty_body() {
    let server = MockServer::with_script(vec![MockResponse::empty()]).await;
    let error = server.api().gateway_bot().await.expect_err("空响应要报错");
    assert!(error.to_string().contains("网关信息返回为空"), "{error}");
}
