use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_api::{ApiError, Outbound, SendOutcome};
use aurorix_bot::{
    webhook_events, Bot, BotConfig, EventSink, Logger, MemoryStats, Options, ReplyTransport,
    StatsRecorder, TokenSource, WebhookIngress, WEBHOOK_MODE_LABEL,
};
use aurorix_core::Target;
use aurorix_gateway::{
    callback_signature, signing_key, BoxFuture, GatewayState, ProtocolEvent, StaticTokenSource,
    SIGNATURE_HEADER, TIMESTAMP_HEADER, WEBHOOK_PATH,
};
use aurorix_plugin::{
    CommandRegistry, CommandSpec, Invocation, Outgoing, PluginCommands, ReplyKind,
};
use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use ed25519_dalek::Signer;
use serde_json::{json, Value};
use tower::ServiceExt;

const SECRET: &str = "secret-webhook";

const WAIT_QUANTUM: Duration = Duration::from_millis(10);

const WAIT_LIMIT: Duration = Duration::from_secs(3);

struct SpySink {
    tx: tokio::sync::mpsc::UnboundedSender<ProtocolEvent>,
}

impl EventSink for SpySink {
    fn dispatch(&self, event: ProtocolEvent) {
        let _ = self.tx.send(event);
    }
}

struct RecordingTransport {
    sent: Mutex<Vec<String>>,
}

impl RecordingTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
        })
    }

    fn contents(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }
}

impl ReplyTransport for RecordingTransport {
    fn send<'a>(
        &'a self,
        target: &'a Target,
        message: &'a Outbound,
    ) -> BoxFuture<'a, Result<SendOutcome, ApiError>> {
        let target = target.clone();
        let content = message.content.clone();
        Box::pin(async move {
            self.sent.lock().unwrap().push(content);
            Ok(SendOutcome {
                message_id: Some(String::from("out-1")),
                audit_id: None,
                target,
                delivered_active: false,
                fallback: false,
            })
        })
    }
}

fn sign(secret: &str, timestamp: &str, body: &str) -> String {
    let key = signing_key(secret);
    let mut message = String::from(timestamp);
    message.push_str(body);
    key.sign(message.as_bytes())
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn quiet_logger() -> Arc<Logger> {
    Logger::new(Options::default().with_console(false))
}

fn messages(logger: &Logger) -> Vec<String> {
    logger
        .recent(64)
        .iter()
        .map(|record| record.msg.clone())
        .collect()
}

fn registry() -> Arc<CommandRegistry> {
    let registry = CommandRegistry::new();
    registry.register_plugin(
        PluginCommands::new("demo", "示例插件").command(
            CommandSpec::new("echo")
                .describe("复读")
                .on_sync(|invocation: &Invocation| {
                    Ok(Some(Outgoing::new(
                        ReplyKind::Handler,
                        format!("echo: {}", invocation.text),
                    )))
                }),
        ),
    );
    Arc::new(registry)
}

fn message_body() -> String {
    json!({
        "op": 0,
        "id": "evt-1",
        "t": "GROUP_AT_MESSAGE_CREATE",
        "s": 3,
        "d": {
            "id": "msg-1",
            "content": "/echo webhook",
            "group_openid": "g1",
            "author": {
                "member_openid": "u1",
                "username": "小明",
                "member_role": "member"
            }
        }
    })
    .to_string()
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("要能探测空闲端口");
    let port = listener.local_addr().expect("本地地址").port();
    drop(listener);
    port
}

fn temp_lock(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("aurorix-webhook-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir.join("aurorix.lock").to_string_lossy().to_string()
}

fn webhook_config(name: &str, port: u16) -> BotConfig {
    let mut config = BotConfig::new("appid-webhook", SECRET)
        .with_intents(vec![String::from("GROUP_AT_MESSAGE_CREATE")])
        .with_admin("127.0.0.1", port)
        .with_lock_file(temp_lock(name));
    config.protocol = String::from("webhook");
    config
}

async fn post(
    app: &Router,
    path: &str,
    signature: Option<&str>,
    timestamp: Option<&str>,
    body: &str,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(signature) = signature {
        builder = builder.header(SIGNATURE_HEADER, signature);
    }
    if let Some(timestamp) = timestamp {
        builder = builder.header(TIMESTAMP_HEADER, timestamp);
    }
    let request = builder
        .body(Body::from(body.to_string()))
        .expect("构造请求");
    let response = app.clone().oneshot(request).await.expect("路由应可调用");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("读响应体")
        .to_vec();
    (status, bytes)
}

async fn raw_post(port: u16, path: &str, signature: &str, timestamp: &str, body: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("webhook 要能连上");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{SIGNATURE_HEADER}: {signature}\r\n{TIMESTAMP_HEADER}: {timestamp}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("请求要能写出");
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await.expect("响应要能读回");
    String::from_utf8_lossy(&buffer).to_string()
}

async fn wait_until<F: Fn() -> bool>(condition: F) {
    let started = std::time::Instant::now();
    while started.elapsed() < WAIT_LIMIT {
        if condition() {
            return;
        }
        tokio::time::sleep(WAIT_QUANTUM).await;
    }
    assert!(condition(), "等待超时: 条件在 {WAIT_LIMIT:?} 内没有成立");
}

#[tokio::test]
async fn callback_validation_returns_plain_token_and_signature() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let ingress = webhook_events(SECRET, Arc::new(SpySink { tx }) as Arc<dyn EventSink>);
    assert_eq!(WebhookIngress::path(), WEBHOOK_PATH, "路径要和网关一致");
    let body = json!({
        "op": 13,
        "d": { "plain_token": "tok-1", "event_ts": "1710000000" }
    })
    .to_string();
    let timestamp = "1700000000";
    let signature = sign(SECRET, timestamp, &body);
    let (status, bytes) = post(
        &ingress.router(),
        WEBHOOK_PATH,
        Some(&signature),
        Some(timestamp),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "回调校验要 200");
    let payload: Value = serde_json::from_slice(&bytes).expect("响应体要是 JSON");
    assert_eq!(
        payload,
        json!({
            "plain_token": "tok-1",
            "signature": callback_signature(SECRET, "1710000000", "tok-1")
        }),
        "回调校验要回原样的 plain_token 与 Ed25519 签名"
    );
    assert!(rx.try_recv().is_err(), "回调校验不该产生事件");
}

#[tokio::test]
async fn signed_message_event_reaches_the_sink() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let ingress = webhook_events(SECRET, Arc::new(SpySink { tx }) as Arc<dyn EventSink>);
    let body = message_body();
    let timestamp = "1700000001";
    let signature = sign(SECRET, timestamp, &body);
    let (status, bytes) = post(
        &ingress.router(),
        WEBHOOK_PATH,
        Some(&signature),
        Some(timestamp),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "验签通过要立刻 200");
    assert!(bytes.is_empty(), "普通事件不该回 body");
    let event = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("事件要进 sink")
        .expect("sink 不该关闭");
    assert_eq!(event.event_type.as_deref(), Some("GROUP_AT_MESSAGE_CREATE"));
    assert_eq!(event.id.as_deref(), Some("evt-1"));
    assert_eq!(event.seq, Some(3));
    assert_eq!(event.data["id"], json!("msg-1"));
    assert_eq!(event.data["content"], json!("/echo webhook"));
}

#[tokio::test]
async fn bad_signature_returns_401_and_reaches_nothing() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let ingress = webhook_events(SECRET, Arc::new(SpySink { tx }) as Arc<dyn EventSink>);
    let app = ingress.router();
    let body = message_body();
    let timestamp = "1700000002";
    let zeros = "0".repeat(128);
    let (status, _) = post(&app, WEBHOOK_PATH, Some(&zeros), Some(timestamp), &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "假签名要 401");
    let wrong = sign("另一个密钥", timestamp, &body);
    let (status, _) = post(&app, WEBHOOK_PATH, Some(&wrong), Some(timestamp), &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "别人签的也要 401");
    let (status, _) = post(&app, WEBHOOK_PATH, None, Some(timestamp), &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "缺签名要 401");
    tokio::task::yield_now().await;
    assert!(rx.try_recv().is_err(), "验签失败的事件一条都不许进 sink");
}

#[tokio::test]
async fn webhook_mode_rejects_intents_that_need_websocket() {
    let mut config = webhook_config("intents", free_port());
    config.intents = vec![String::from("AT_MESSAGE_CREATE")];
    let bot = Bot::builder(config)
        .with_transport(RecordingTransport::new() as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry())
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(quiet_logger())
        .with_signal_handling(false)
        .build()
        .expect("装配失败");
    let error = bot.start().await.expect_err("频道事件必须被拒绝");
    let text = error.to_string();
    assert!(
        text.contains("webhook 模式接不住当前 intents"),
        "错误要说明 webhook 接不住: {text}"
    );
    assert!(text.contains("WebSocket"), "错误要指出只有 WS 能收: {text}");
}

#[tokio::test]
async fn webhook_mode_feeds_signed_events_into_the_pipeline() {
    let port = free_port();
    let transport = RecordingTransport::new();
    let logger = quiet_logger();
    let bot = Bot::builder(webhook_config("pipeline", port))
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry())
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(Arc::clone(&logger))
        .with_signal_handling(false)
        .build()
        .expect("装配失败");
    let handle = bot.start().await.expect("webhook 模式应能启动");
    assert!(bot.is_webhook(), "protocol=webhook 要走 webhook 分支");
    assert_eq!(
        bot.gateway_state(),
        GatewayState::Disconnected,
        "webhook 模式不该启动 WebSocket 网关"
    );
    assert!(
        messages(&logger)
            .iter()
            .any(|line| line.contains(WEBHOOK_MODE_LABEL) && line.contains(WEBHOOK_PATH)),
        "启动要打一行中文说明带回调路径: {:?}",
        messages(&logger)
    );
    let body = message_body();
    let timestamp = "1700000003";
    let signature = sign(SECRET, timestamp, &body);
    let response = raw_post(port, WEBHOOK_PATH, &signature, timestamp, &body).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "webhook 要回 200: {response}"
    );
    wait_until(|| !transport.contents().is_empty()).await;
    assert_eq!(transport.contents(), vec![String::from("echo: webhook")]);
    assert_eq!(
        handle.counters().received,
        1,
        "webhook 事件要和 WS 事件走同一条处理链"
    );
    let report = bot
        .shutdown(Duration::from_secs(3))
        .await
        .expect("停机失败");
    assert_eq!(report.live_tasks, 0, "停机后不许留下后台任务");
    assert_eq!(bot.live_tasks(), 0);
}

async fn raw_get(port: u16, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("要能连上");
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("请求要能写出");
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await.expect("响应要能读回");
    String::from_utf8_lossy(&buffer).to_string()
}

#[tokio::test]
async fn webhook_mode_shares_the_admin_http_server() {
    let port = free_port();
    let transport = RecordingTransport::new();
    let mut config = webhook_config("admin", port);
    config.admin_password = String::from("pwd-webhook");
    let bot = Bot::builder(config)
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry())
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(quiet_logger())
        .with_signal_handling(false)
        .build()
        .expect("装配失败");
    let handle = bot.start().await.expect("webhook 与管理台应能同时启动");
    let body = message_body();
    let timestamp = "1700000004";
    let signature = sign(SECRET, timestamp, &body);
    let response = raw_post(port, WEBHOOK_PATH, &signature, timestamp, &body).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "webhook 要回 200: {response}"
    );
    let admin = raw_get(port, "/admin/api/overview").await;
    assert!(
        admin.starts_with("HTTP/1.1 401"),
        "同端口上的管理台要照常鉴权: {admin}"
    );
    wait_until(|| !transport.contents().is_empty()).await;
    assert_eq!(transport.contents(), vec![String::from("echo: webhook")]);
    let report = bot
        .shutdown(Duration::from_secs(3))
        .await
        .expect("停机失败");
    assert_eq!(report.live_tasks, 0, "停机后不许留下后台任务");
    drop(handle);
}
