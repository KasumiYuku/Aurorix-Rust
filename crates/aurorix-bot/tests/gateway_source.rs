use std::sync::Arc;
use std::time::Duration;

use aurorix_api::{BotApi, BotConfig, GatewayBotInfo, SessionStartLimit};
use aurorix_bot::{
    Logger, OfficialGatewaySource, GATEWAY_INFO_FAILURE_DELAY, GATEWAY_SESSION_DELAY,
};
use aurorix_gateway::GatewaySource;
use aurorix_log::{Level, Options};
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::Response;
use axum::Router;

const EXHAUSTED: &str = r#"{"url":"wss://official.example","shards":4,
"session_start_limit":{"total":1000,"remaining":0,"reset_after":120000,"max_concurrency":1}}"#;

const AVAILABLE: &str = r#"{"url":"wss://official.example","shards":2,
"session_start_limit":{"total":1000,"remaining":7,"reset_after":60000,"max_concurrency":1}}"#;

async fn serve(body: &'static str) -> String {
    let app = Router::new()
        .fallback(handle)
        .with_state(Arc::<str>::from(body));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("监听随机端口");
    let addr = listener.local_addr().expect("取本地地址");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

async fn handle(State(body): State<Arc<str>>, uri: Uri) -> Response {
    let payload = match uri.path().ends_with("getAppAccessToken") {
        true => Arc::<str>::from(r#"{"access_token":"tok-1","expires_in":"7200"}"#),
        false => body,
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .expect("构造响应")
}

fn source(base: &str) -> OfficialGatewaySource {
    let api = Arc::new(BotApi::new(
        BotConfig::new("app", "secret")
            .with_base_url(base.to_string())
            .with_token_url(format!("{base}/app/getAppAccessToken")),
    ));
    OfficialGatewaySource::new(api, Logger::new(Options::default().with_level(Level::Error)))
}

fn info(remaining: i64, reset_after: i64) -> GatewayBotInfo {
    GatewayBotInfo {
        url: String::from("wss://official.example"),
        shards: 3,
        session_start_limit: SessionStartLimit {
            total: 1000,
            remaining,
            reset_after,
            max_concurrency: 1,
        },
    }
}

#[tokio::test]
async fn endpoint_comes_from_the_gateway_api() {
    let base = serve(EXHAUSTED).await;
    let endpoint = source(&base)
        .endpoint()
        .await
        .expect("要能取到网关地址");
    assert_eq!(endpoint.url, "wss://official.example");
    assert_eq!(endpoint.shards, 4);
    assert_eq!(endpoint.shard(), [0, 4]);
}

#[tokio::test]
async fn exhausted_quota_waits_for_the_reset_window() {
    let base = serve(EXHAUSTED).await;
    let delay = source(&base).reauth_delay().await;
    assert_eq!(
        delay,
        Duration::from_secs(120),
        "配额耗尽要等满 reset_after, 不能写死 3 秒"
    );
}

#[tokio::test]
async fn remaining_quota_uses_the_short_session_delay() {
    let base = serve(AVAILABLE).await;
    let delay = source(&base).reauth_delay().await;
    assert!(
        delay >= Duration::from_secs(2) && delay <= Duration::from_secs(4),
        "有余额时按 {GATEWAY_SESSION_DELAY:?} 抖动退避, 实际 {delay:?}"
    );
}

#[tokio::test]
async fn reauth_delay_for_follows_the_quota() {
    assert_eq!(
        OfficialGatewaySource::reauth_delay_for(&info(0, 90_000)),
        Duration::from_secs(90)
    );
    let jittered = OfficialGatewaySource::reauth_delay_for(&info(5, 90_000));
    assert!(
        jittered >= Duration::from_secs(2) && jittered <= Duration::from_secs(4),
        "有余额时走会话退避, 实际 {jittered:?}"
    );
    assert_eq!(
        GATEWAY_INFO_FAILURE_DELAY,
        Duration::from_secs(5),
        "查配额失败也要有个确定的退避量"
    );
}
