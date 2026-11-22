mod support;

use std::sync::Arc;
use std::time::Duration;

use aurorix_api::{ApiError, BotApi};
use support::{token_ok, MockResponse, MockServer, Recorded, ACCESS_TOKEN, TOKEN_PATH};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hundred_callers_hit_token_endpoint_once() {
    let server = MockServer::start(|record: &Recorded, _index: usize| {
        if record.path.ends_with(TOKEN_PATH) {
            std::thread::sleep(Duration::from_millis(60));
            return token_ok();
        }
        MockResponse::ok_id("m1")
    })
    .await;
    let api = Arc::new(server.api());
    let mut handles = Vec::new();
    for _ in 0..100 {
        let clone = Arc::clone(&api);
        handles.push(tokio::spawn(async move { clone.access_token().await }));
    }
    let mut tokens = Vec::new();
    for handle in handles {
        tokens.push(
            handle
                .await
                .expect("任务不能 panic")
                .expect("取 token 要成功"),
        );
    }
    assert_eq!(server.count_token(), 1, "并发 100 次只能打 1 次 token 端点");
    assert!(
        tokens.iter().all(|token| token == ACCESS_TOKEN),
        "所有调用者拿到同一 token"
    );
}

#[tokio::test]
async fn invalidate_forces_a_refetch() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    assert_eq!(api.access_token().await.unwrap(), ACCESS_TOKEN);
    assert_eq!(server.count_token(), 1);
    api.invalidate_token();
    assert_eq!(api.access_token().await.unwrap(), ACCESS_TOKEN);
    assert_eq!(server.count_token(), 2, "invalidate 之后必须重新取");
}

#[tokio::test]
async fn cached_token_is_reused_without_second_request() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    for _ in 0..5 {
        api.access_token().await.unwrap();
    }
    assert_eq!(server.count_token(), 1, "缓存命中不该再打端点");
}

#[tokio::test]
async fn token_failure_is_reported() {
    let shared = support::Scripted::with_token(
        Vec::new(),
        MockResponse::status(401, r#"{"code":100016,"message":"secret 输入错误"}"#),
    );
    let server = MockServer::with_scripted(shared).await;
    let api = server.api();
    let err = api.access_token().await.unwrap_err();
    match &err {
        ApiError::Status { status, .. } => assert_eq!(*status, 401),
        other => panic!("应是状态错误: {other}"),
    }
}

#[tokio::test]
async fn send_carries_bot_authorization_header() {
    let server = MockServer::with_script(Vec::new()).await;
    let api: BotApi = server.api();
    let target = aurorix_api::Target::new(aurorix_api::Scene::Group, "g1");
    api.send(&target, &aurorix_api::Outbound::text("x"))
        .await
        .unwrap();
    let request = server.last();
    assert_eq!(
        request.header("authorization"),
        format!("QQBot {ACCESS_TOKEN}")
    );
    assert!(request
        .header("content-type")
        .starts_with("application/json"));
}
