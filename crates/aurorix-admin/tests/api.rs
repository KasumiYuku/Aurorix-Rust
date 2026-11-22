use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aurorix_admin::{
    clamp_limit, router, AdminState, ApiError, Counts, GatewayStatus, LogSource, PluginInfo,
    PluginSource, SessionStore, StatsSource, StatusSource, StatusView, DEFAULT_LOG_LIMIT,
    MAX_LOG_LIMIT,
};
use aurorix_db::{AppConfig, StatsView};
use aurorix_log::{Level, Logger, Options};
use axum::body::Bytes;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

const PASSWORD: &str = "sup3r-secret-pwd";

const APP_SECRET: &str = "app-s3cret";

const UNKNOWN_FIELD: &str = "custom_unknown";

struct MockStatus;

impl StatusSource for MockStatus {
    fn snapshot(&self) -> StatusView {
        StatusView {
            uptime_seconds: 4242,
            version: "9.9.9".to_string(),
            protocol: "websocket".to_string(),
            port: 8080,
            tasks: 12,
            gateway: GatewayStatus {
                connected: true,
                session_id: "sess-1".to_string(),
                seq: 77,
                heartbeat_ack: true,
                since_ms: 111,
                bot_id: "bot-1".to_string(),
                bot_name: "小助手".to_string(),
                shard_id: 0,
                shards: 1,
            },
            counts: Counts {
                plugins: 3,
                commands: 12,
                jobs: 2,
                templates: 8,
            },
        }
    }
}

struct MockStats;

impl StatsSource for MockStats {
    fn view(&self) -> StatsView {
        StatsView {
            groups: 5,
            peers: 6,
            friends: 7,
            channels: 8,
            total_recv: 100,
            total_sent: 90,
            ..StatsView::default()
        }
    }
}

struct MockPlugins;

impl PluginSource for MockPlugins {
    fn plugins(&self) -> Vec<PluginInfo> {
        vec![
            PluginInfo {
                id: "arcaea".to_string(),
                name: "Arcaea".to_string(),
                version: "1.0.0".to_string(),
                enabled: true,
                commands: vec![String::from("a"), String::from("b")],
                ..PluginInfo::default()
            },
            PluginInfo {
                id: "phigros".to_string(),
                name: "Phigros".to_string(),
                version: "2.1.0".to_string(),
                enabled: false,
                ..PluginInfo::default()
            },
            PluginInfo {
                id: "rotaeno".to_string(),
                name: "Rotaeno".to_string(),
                version: "0.4.2".to_string(),
                enabled: true,
                ..PluginInfo::default()
            },
        ]
    }
}

struct Fixture {
    state: AdminState,
    logger: Arc<Logger>,
    path: PathBuf,
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("aurorix-admin-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("config.json");
    let raw = format!(
        r#"{{"port":8080,"appid":"app-1","secret":"{APP_SECRET}","admin_password":"{PASSWORD}","{UNKNOWN_FIELD}":{{"keep":1}}}}"#
    );
    std::fs::write(&path, raw).expect("写配置文件");
    let config = AppConfig::load(&path).expect("读配置文件");
    let logger = Logger::new(
        Options::default()
            .with_level(Level::Debug)
            .with_console(false),
    );
    let logs: Arc<dyn LogSource> = logger.clone();
    let state = AdminState::new(&path, config)
        .with_status(Arc::new(MockStatus))
        .with_stats(Arc::new(MockStats))
        .with_plugins(Arc::new(MockPlugins))
        .with_logs(logs)
        .with_logger(logger.clone());
    Fixture {
        state,
        logger,
        path,
    }
}

fn request(method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Bytes> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Bytes::from(value.to_string()))
            .expect("构造请求"),
        None => builder.body(Bytes::new()).expect("构造请求"),
    }
}

async fn call(app: &Router, request: Request<Bytes>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = request.map(axum::body::Body::from);
    let response = app.clone().oneshot(request).await.expect("路由应可调用");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("读响应体");
    (status, headers, bytes.to_vec())
}

fn json_body(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("响应体需为 JSON")
}

fn content_type(headers: &HeaderMap) -> String {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

async fn login(app: &Router) -> String {
    let body = json!({ "admin_password": PASSWORD });
    let (status, _, bytes) = call(
        app,
        request(Method::POST, "/admin/api/login", None, Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "正确密码应登录成功");
    json_body(&bytes)["token"]
        .as_str()
        .expect("登录需返回 token")
        .to_string()
}

async fn next_chunk<S, E>(stream: &mut S) -> Option<Result<Bytes, E>>
where
    S: tokio_stream::Stream<Item = Result<Bytes, E>> + Unpin,
{
    std::future::poll_fn(|context| std::pin::Pin::new(&mut *stream).poll_next(context)).await
}

#[tokio::test]
async fn login_with_correct_password_issues_token() {
    let fx = fixture("login-ok");
    let app = router(fx.state.clone());
    let body = json!({ "admin_password": PASSWORD });
    let (status, headers, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/login", None, Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "正确密码需返回 200");
    assert_eq!(
        content_type(&headers),
        "application/json",
        "登录响应需为 JSON"
    );
    let value = json_body(&bytes);
    assert_eq!(value["ok"], json!(true), "需返回 ok 标记");
    let token = value["token"].as_str().expect("需返回 token");
    assert_eq!(
        token.len(),
        43,
        "32 字节随机数 base64url 无填充应为 43 字符"
    );
    assert_eq!(
        value["expires_in"],
        json!(86400),
        "默认过期时间需为 24 小时"
    );
}

#[tokio::test]
async fn login_accepts_go_password_field_name() {
    let fx = fixture("login-alias");
    let app = router(fx.state.clone());
    let body = json!({ "password": PASSWORD });
    let (status, _, _) = call(
        &app,
        request(Method::POST, "/admin/api/login", None, Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "Go 原版字段名 password 需兼容");
}

#[tokio::test]
async fn login_with_wrong_password_returns_401() {
    let fx = fixture("login-bad");
    let app = router(fx.state.clone());
    let body = json!({ "admin_password": "错误口令" });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/login", None, Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "错误密码需 401");
    assert_eq!(
        json_body(&bytes)["error"],
        json!("密码错误"),
        "错误信息需为中文"
    );
    assert_eq!(fx.state.sessions.len(), 0, "失败登录不应发放会话");
}

#[tokio::test]
async fn status_requires_bearer_token() {
    let fx = fixture("status-auth");
    let app = router(fx.state.clone());
    let (status, _, bytes) =
        call(&app, request(Method::GET, "/admin/api/status", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "无 token 需 401");
    assert_eq!(
        json_body(&bytes)["error"],
        json!("未登录或会话已失效"),
        "错误信息需为中文"
    );
    let (status, _, _) = call(
        &app,
        request(Method::GET, "/admin/api/status", Some("bad-token"), None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "无效 token 需 401");
}

#[tokio::test]
async fn status_with_token_returns_full_fields() {
    let fx = fixture("status-ok");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    fx.logger.error("gateway", "一条错误");
    let (status, headers, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/status", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "带 token 需 200");
    assert_eq!(content_type(&headers), "application/json", "需 JSON 响应");
    let value = json_body(&bytes);
    assert_eq!(
        value["runtime"]["uptime_sec"],
        json!(4242),
        "需返回运行时长"
    );
    assert_eq!(value["runtime"]["version"], json!("9.9.9"), "需返回版本");
    assert_eq!(value["runtime"]["tasks"], json!(12), "需返回后台任务数");
    assert!(
        value["runtime"]["mem"]["rss_bytes"].as_u64().is_some(),
        "内存占用要给出实际字节数"
    );
    assert_eq!(
        value["runtime"]["protocol"],
        json!("websocket"),
        "需返回协议模式"
    );
    assert_eq!(value["runtime"]["port"], json!(8080), "需返回管理台端口");
    assert!(
        value["runtime"]["counters"]["recv"].is_number(),
        "需返回收发计数, 前端概览直接读取"
    );
    assert!(
        value["stats"]["today"].is_object(),
        "需返回今日统计, 前端概览直接读取"
    );
    assert_eq!(value["gateway"]["connected"], json!(true), "需返回连接状态");
    assert_eq!(
        value["gateway"]["session_id"],
        json!("sess-1"),
        "需返回会话 id"
    );
    assert_eq!(value["gateway"]["seq"], json!(77), "需返回序号");
    assert_eq!(value["counts"]["plugins"], json!(3), "需返回插件数");
    assert_eq!(value["counts"]["commands"], json!(12), "需返回指令数");
    assert_eq!(value["logs"]["total"], json!(2), "需返回日志总数");
    assert_eq!(value["logs"]["errors"], json!(1), "需返回错误条数");
}

#[tokio::test]
async fn logs_endpoint_returns_newest_first() {
    let fx = fixture("logs-order");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    for index in 0..3 {
        fx.logger.info("gateway", &format!("第 {index} 条"));
    }
    fx.logger.info("storage", "慢查询");
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/logs?limit=2", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "日志接口需 200");
    let value = json_body(&bytes);
    let entries = value["entries"].as_array().expect("需返回 entries");
    assert_eq!(entries.len(), 2, "limit 需生效");
    assert_eq!(entries[0]["msg"], json!("慢查询"), "第一条需为最新");
    assert_eq!(entries[0]["scope"], json!("storage"), "需带模块名");
    assert_eq!(entries[0]["level"], json!("INFO"), "级别需大写");
    assert_eq!(entries[1]["msg"], json!("第 2 条"), "顺序需为最新优先");
    assert_eq!(
        value["scopes"],
        json!(["admin", "gateway", "storage"]),
        "需返回模块列表"
    );
    assert_eq!(
        value["summary"]["levels"]["info"],
        json!(5),
        "需返回级别汇总"
    );
    assert_eq!(value["total"], json!(5), "需返回累计条数");
}

#[tokio::test]
async fn logs_endpoint_clamps_limit() {
    let fx = fixture("logs-limit");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    for index in 0..3 {
        fx.logger.info("gateway", &format!("第 {index} 条"));
    }
    let (_, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/logs?limit=0", Some(&token), None),
    )
    .await;
    let value = json_body(&bytes);
    assert_eq!(
        value["entries"].as_array().map(Vec::len),
        Some(4),
        "limit 为 0 需回落到默认值并返回全部条目"
    );
    let (_, _, bytes) = call(
        &app,
        request(
            Method::GET,
            "/admin/api/logs?limit=99999",
            Some(&token),
            None,
        ),
    )
    .await;
    let value = json_body(&bytes);
    assert_eq!(
        value["entries"].as_array().map(Vec::len),
        Some(4),
        "limit 超上限需回落到默认值"
    );
}

#[tokio::test]
async fn logs_stream_emits_data_frame_and_unsubscribes() {
    let fx = fixture("stream");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let request = request(Method::GET, "/admin/api/logs/stream", Some(&token), None)
        .map(axum::body::Body::from);
    let response = app.clone().oneshot(request).await.expect("路由应可调用");
    assert_eq!(response.status(), StatusCode::OK, "SSE 需 200");
    assert!(
        content_type(response.headers()).starts_with("text/event-stream"),
        "SSE 响应需 text/event-stream"
    );
    assert_eq!(fx.state.live.subscribers(), 1, "SSE 需建立订阅");
    let mut stream = response.into_body().into_data_stream();
    fx.state.live.publish(
        aurorix_admin::TOPIC_LOG,
        json!({ "module": "gateway", "message": "推送一条" }),
    );
    let chunk = tokio::time::timeout(Duration::from_secs(2), next_chunk(&mut stream))
        .await
        .expect("等待 SSE 帧超时")
        .expect("SSE 流不应提前结束")
        .expect("读取 SSE 帧失败");
    let text = String::from_utf8_lossy(&chunk);
    assert!(text.contains("data:"), "SSE 帧需包含 data 字段");
    assert!(text.contains("推送一条"), "SSE 帧需包含日志正文");
    assert!(text.contains("log.entry"), "SSE 帧需带日志主题");
    drop(stream);
    assert_eq!(fx.state.live.subscribers(), 0, "客户端断开需退订");
}

#[tokio::test]
async fn live_stream_pushes_inbox_events() {
    let fx = fixture("live-inbox");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let request =
        request(Method::GET, "/admin/api/stream", Some(&token), None).map(axum::body::Body::from);
    let response = app.clone().oneshot(request).await.expect("路由应可调用");
    assert_eq!(response.status(), StatusCode::OK, "SSE 需 200");
    assert!(
        content_type(response.headers()).starts_with("text/event-stream"),
        "SSE 响应需 text/event-stream"
    );
    let mut stream = response.into_body().into_data_stream();
    assert_eq!(fx.state.live.subscribers(), 1, "SSE 需建立订阅");
    fx.state.live.publish(
        aurorix_admin::TOPIC_INBOX,
        json!({ "conv": "g1", "content": "新消息" }),
    );
    let chunk = tokio::time::timeout(Duration::from_secs(2), next_chunk(&mut stream))
        .await
        .expect("等待 SSE 帧超时")
        .expect("SSE 流不应提前结束")
        .expect("读取 SSE 帧失败");
    let text = String::from_utf8_lossy(&chunk);
    assert!(text.contains("inbox.message"), "SSE 帧需带主题: {text}");
    assert!(text.contains("新消息"), "SSE 帧需带消息内容: {text}");
}

#[tokio::test]
async fn config_write_then_read_roundtrip() {
    let fx = fixture("config-write");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "log_level": "debug" } });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "合法写入需 200");
    let value = json_body(&bytes);
    assert_eq!(value["ok"], json!(true), "需返回 ok");
    assert_eq!(
        value["restart_needed"],
        json!(false),
        "log_level 属热更字段"
    );
    assert_eq!(value["hot_fields"], json!(["log_level"]), "需回报热更字段");
    assert_eq!(value["restart_fields"], json!([]), "不应有重启字段");
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/config", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "读取配置需 200");
    let value = json_body(&bytes);
    let fields = value["fields"].as_array().expect("需返回字段表");
    let log_level = fields
        .iter()
        .find(|field| field["key"] == json!("log_level"))
        .expect("需返回日志级别字段");
    assert_eq!(log_level["value"], json!("debug"), "写入后需能读回");
    let port = fields
        .iter()
        .find(|field| field["key"] == json!("port"))
        .expect("需返回监听端口字段");
    assert_eq!(port["value"], json!(8080), "其它字段需保持不变");
}

#[tokio::test]
async fn config_rejects_invalid_value_in_chinese() {
    let fx = fixture("config-invalid");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "log_level": "trace" } });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "非法值需 400");
    let message = json_body(&bytes)["error"]
        .as_str()
        .expect("需返回中文错误")
        .to_string();
    assert!(
        message.contains("log_level") && message.contains("debug/info/warn/error"),
        "错误信息需指明可选取值"
    );
    assert!(!message.is_ascii(), "错误信息需为中文");
    let raw = std::fs::read_to_string(&fx.path).expect("读配置");
    let value: Value = serde_json::from_str(&raw).expect("解析配置");
    assert!(value.get("log_level").is_none(), "校验失败不得写盘");
}

#[tokio::test]
async fn config_rejects_unknown_key() {
    let fx = fixture("config-unknown");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "glow": 1 } });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "未知 key 需 400");
    assert_eq!(
        json_body(&bytes)["error"],
        json!("不支持修改的配置项: glow"),
        "错误信息需为中文并指出 key"
    );
}

#[test]
fn clamp_limit_bounds_are_inclusive() {
    assert_eq!(clamp_limit(None), DEFAULT_LOG_LIMIT, "缺省需用默认值");
    assert_eq!(clamp_limit(Some(0)), DEFAULT_LOG_LIMIT, "0 需用默认值");
    assert_eq!(clamp_limit(Some(-5)), DEFAULT_LOG_LIMIT, "负数需用默认值");
    assert_eq!(
        clamp_limit(Some(MAX_LOG_LIMIT as i64)),
        MAX_LOG_LIMIT,
        "上限值本身需保留"
    );
    assert_eq!(
        clamp_limit(Some(MAX_LOG_LIMIT as i64 + 1)),
        DEFAULT_LOG_LIMIT,
        "超上限需用默认值"
    );
    assert_eq!(clamp_limit(Some(1)), 1, "区间内需原样保留");
}

#[tokio::test]
async fn config_ignores_empty_secrets_and_applies_new_ones() {
    let fx = fixture("config-secret");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "admin_password": "", "secret": "" } });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "空密钥需被忽略");
    let value = json_body(&bytes);
    assert_eq!(value["hot_fields"], json!([]), "空密钥不应计入热更字段");
    assert_eq!(value["restart_fields"], json!([]), "空密钥不应计入重启字段");
    assert_eq!(fx.state.password(), PASSWORD, "空密钥不得清空原值");
    let token = login(&app).await;
    let body = json!({ "values": { "admin_password": "新的口令" } });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "新密钥需写入");
    assert_eq!(
        json_body(&bytes)["hot_fields"],
        json!(["admin_password"]),
        "admin_password 属热更字段"
    );
    assert_eq!(fx.state.password(), "新的口令", "新密钥需生效");
}

#[tokio::test]
async fn config_preserves_unknown_file_fields() {
    let fx = fixture("config-preserve");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "log_level": "debug", "port": 9090 } });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "写入需成功");
    assert_eq!(
        json_body(&bytes)["restart_needed"],
        json!(true),
        "port 属重启字段"
    );
    let raw = std::fs::read_to_string(&fx.path).expect("读配置");
    let value: Value = serde_json::from_str(&raw).expect("解析配置");
    assert_eq!(value["log_level"], json!("debug"), "指定 key 需落盘");
    assert_eq!(value["port"], json!(9090), "指定 key 需落盘");
    assert_eq!(
        value[UNKNOWN_FIELD]["keep"],
        json!(1),
        "未知字段必须原样保留"
    );
    assert_eq!(value["appid"], json!("app-1"), "未指定字段需原样保留");
}

#[tokio::test]
async fn plugins_returns_injected_entries() {
    let fx = fixture("plugins");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/plugins", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "插件接口需 200");
    let plugins = json_body(&bytes);
    let list = plugins.as_array().expect("需返回插件数组");
    assert_eq!(list.len(), 3, "需返回注入的 3 个插件");
    assert_eq!(list[0]["id"], json!("arcaea"), "需返回插件 id");
    assert_eq!(list[0]["name"], json!("Arcaea"), "需返回插件名");
    assert_eq!(list[0]["version"], json!("1.0.0"), "需返回插件版本");
    assert_eq!(list[0]["enabled"], json!(true), "需返回启用状态");
    assert_eq!(list[1]["enabled"], json!(false), "需返回禁用状态");
    assert_eq!(
        list[0]["commands"],
        json!(["a", "b"]),
        "管理台表格直接读 commands.length"
    );
    assert_eq!(
        list[0]["fields"],
        json!([]),
        "管理台表格直接读 fields.length"
    );
    assert!(
        list[0]["values"].is_object(),
        "插件详情要拿到配置对象, 不能是 null"
    );
    assert_eq!(
        list[0]["access"]["disabled"],
        json!(false),
        "插件表格读 access.disabled"
    );
    assert!(
        list[0]["access"]["maintenance"].is_object(),
        "插件详情读 access.maintenance"
    );
}

#[tokio::test]
async fn stats_returns_injected_view() {
    let fx = fixture("stats");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/stats", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "统计接口需 200");
    let value = json_body(&bytes);
    assert_eq!(value["groups"], json!(5), "需返回注入的群数");
    assert_eq!(value["total_recv"], json!(100), "需返回注入的收消息总数");
    assert_eq!(value["total_sent"], json!(90), "需返回注入的发消息总数");
}

#[tokio::test]
async fn unknown_path_returns_chinese_json_error() {
    let fx = fixture("fallback");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, headers, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/nope", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "未知路径需 404");
    assert_eq!(
        content_type(&headers),
        "application/json",
        "错误响应需 JSON"
    );
    assert_eq!(
        json_body(&bytes)["error"],
        json!("接口不存在"),
        "错误信息需为中文"
    );
}

#[tokio::test]
async fn responses_and_logs_never_leak_secrets() {
    let fx = fixture("no-leak");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "log_level": "info" } });
    let (status, _, _) = call(
        &app,
        request(Method::POST, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "改配置需成功");
    for uri in [
        "/admin/api/status",
        "/admin/api/config",
        "/admin/api/plugins",
        "/admin/api/logs",
        "/admin/api/stats",
    ] {
        let (_, _, bytes) = call(&app, request(Method::GET, uri, Some(&token), None)).await;
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(PASSWORD), "{uri} 响应不得含管理密码明文");
        assert!(!text.contains(APP_SECRET), "{uri} 响应不得含 AppSecret");
        assert!(!text.contains(&token), "{uri} 响应不得含会话 token");
    }
    let records = fx.logger.recent(50);
    assert!(!records.is_empty(), "登录与改配置需留下日志");
    for record in records {
        assert!(!record.msg.contains(PASSWORD), "日志正文不得含管理密码");
        assert!(!record.msg.contains(&token), "日志正文不得含会话 token");
        for (key, value) in &record.fields {
            assert!(!value.contains(PASSWORD), "日志字段不得含管理密码");
            assert!(!key.contains(PASSWORD), "日志字段名不得含管理密码");
        }
    }
}

#[test]
fn session_store_expires_tokens() {
    let store = SessionStore::new(Duration::from_millis(20));
    let (token, expires_in) = store.issue();
    assert_eq!(expires_in, 0, "毫秒级 TTL 折算为 0 秒");
    assert!(store.validate(&token), "刚签发的 token 需有效");
    std::thread::sleep(Duration::from_millis(40));
    assert!(!store.validate(&token), "过期 token 需失效");
    assert_eq!(
        SessionStore::default().ttl_seconds(),
        86400,
        "默认 TTL 为 24 小时"
    );
}

#[test]
fn exhausted_session_store_reports_empty() {
    let store = SessionStore::default();
    assert!(store.is_empty(), "新建会话表需为空");
    let (token, _) = store.issue();
    assert_eq!(store.len(), 1, "签发后需有一个会话");
    store.revoke(&token);
    assert!(store.is_empty(), "注销后需为空");
}

#[test]
fn api_error_maps_status_and_message() {
    let error = ApiError::BadRequest("请求格式无效".to_string());
    assert_eq!(error.status(), StatusCode::BAD_REQUEST, "400 需对应错误");
    assert_eq!(error.message(), "请求格式无效", "需保留中文信息");
    assert_eq!(
        ApiError::Unauthorized("未登录".to_string()).status(),
        StatusCode::UNAUTHORIZED,
        "401 需对应错误"
    );
    assert_eq!(
        ApiError::Unavailable("未启用".to_string()).status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "503 需对应错误"
    );
    assert_eq!(
        ApiError::Server("保存配置失败".to_string()).status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "500 需对应错误"
    );
}

fn cookie_token(headers: &HeaderMap) -> String {
    let raw = headers
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("登录需下发 px_admin Cookie");
    assert!(
        raw.contains("HttpOnly") && raw.contains("Path=/"),
        "Cookie 需带 HttpOnly 与 Path=/: {raw}"
    );
    raw.split(';')
        .next()
        .and_then(|pair| pair.trim().strip_prefix("px_admin="))
        .expect("Cookie 名需为 px_admin")
        .to_string()
}

fn cookie_request(method: Method, uri: &str, token: &str) -> Request<Bytes> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, format!("px_admin={token}"))
        .body(Bytes::new())
        .expect("构造请求")
}

#[tokio::test]
async fn go_admin_api_paths_accept_the_session_cookie() {
    let fx = fixture("go-paths");
    let app = router(fx.state.clone());
    let body = json!({ "password": PASSWORD });
    let (status, headers, _) = call(
        &app,
        request(Method::POST, "/admin/api/login", None, Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = cookie_token(&headers);
    for uri in [
        "/admin/api/me",
        "/admin/api/overview",
        "/admin/api/status",
        "/admin/api/logs",
        "/admin/api/stats",
        "/admin/api/config",
        "/admin/api/plugins",
    ] {
        let (status, _, _) = call(&app, cookie_request(Method::GET, uri, &token)).await;
        assert_eq!(status, StatusCode::OK, "{uri} 需用 Cookie 直接放行");
    }
    let (status, _, bytes) = call(&app, cookie_request(Method::GET, "/admin/api/me", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["admin"], json!(true));
}

#[tokio::test]
async fn go_admin_api_paths_reject_missing_session() {
    let fx = fixture("go-paths-denied");
    let app = router(fx.state.clone());
    for uri in ["/admin/api/me", "/admin/api/overview", "/admin/api/logs"] {
        let (status, _, _) = call(&app, request(Method::GET, uri, None, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri} 未带会话需 401");
    }
}

#[tokio::test]
async fn logout_clears_cookie_and_revokes_session() {
    let fx = fixture("logout");
    let app = router(fx.state.clone());
    let body = json!({ "password": PASSWORD });
    let (_, headers, _) = call(
        &app,
        request(Method::POST, "/admin/api/login", None, Some(body)),
    )
    .await;
    let token = cookie_token(&headers);
    let (status, cleared, _) = call(
        &app,
        cookie_request(Method::POST, "/admin/api/logout", &token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let raw = cleared
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("登出需清除 Cookie");
    assert!(raw.contains("Max-Age=0"), "登出需把 Max-Age 置 0: {raw}");
    let (status, _, _) = call(&app, cookie_request(Method::GET, "/admin/api/me", &token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "登出后旧会话需失效");
}

#[tokio::test]
async fn remember_extends_cookie_lifetime() {
    let fx = fixture("remember");
    let app = router(fx.state.clone());
    let short = json!({ "password": PASSWORD });
    let (_, headers, _) = call(
        &app,
        request(Method::POST, "/admin/api/login", None, Some(short)),
    )
    .await;
    let raw = headers
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("Cookie");
    assert!(
        raw.contains(&format!("Max-Age={}", fx.state.sessions.ttl_seconds())),
        "默认 Cookie 需用会话 TTL: {raw}"
    );
    let long = json!({ "password": PASSWORD, "remember": true });
    let (_, headers, _) = call(
        &app,
        request(Method::POST, "/admin/api/login", None, Some(long)),
    )
    .await;
    let raw = headers
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("Cookie");
    assert!(
        raw.contains(&format!("Max-Age={}", aurorix_admin::REMEMBER_TTL_SECONDS)),
        "记住登录需用 30 天 TTL: {raw}"
    );
}
