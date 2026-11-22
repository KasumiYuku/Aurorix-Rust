use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aurorix_admin::{router, AdminState, ConfigListener};
use aurorix_db::AppConfig;
use aurorix_log::{Logger, Options};
use axum::body::Bytes;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

const PASSWORD: &str = "持久化口令-Test1";

struct Recorder {
    seen: Mutex<Vec<AppConfig>>,
}

impl Recorder {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
        })
    }

    fn last(&self) -> Option<AppConfig> {
        self.seen.lock().expect("锁").last().cloned()
    }
}

impl ConfigListener for Recorder {
    fn applied(&self, config: &AppConfig) {
        self.seen.lock().expect("锁").push(config.clone());
    }
}

fn write_config(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aurorix-admin-persist-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("config.json");
    std::fs::write(
        &path,
        format!(r#"{{"port":8080,"appid":"app-1","secret":"s","admin_password":"{PASSWORD}"}}"#),
    )
    .expect("写配置文件");
    path
}

fn state(path: &Path) -> AdminState {
    let config = AppConfig::load(path).expect("读配置文件");
    AdminState::new(path, config).with_logger(Logger::new(Options::default().with_console(false)))
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

async fn call(app: &Router, request: Request<Bytes>) -> (StatusCode, Vec<u8>) {
    let request = request.map(axum::body::Body::from);
    let response = app.clone().oneshot(request).await.expect("路由应可调用");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("读响应体");
    (status, bytes.to_vec())
}

fn json_body(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("响应体需为 JSON")
}

async fn login(app: &Router) -> String {
    let body = json!({ "password": PASSWORD });
    let (status, bytes) = call(
        app,
        request(Method::POST, "/admin/api/login", None, Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "正确口令应登录成功");
    json_body(&bytes)["token"]
        .as_str()
        .expect("登录需返回 token")
        .to_string()
}

fn field_value(view: &Value, key: &str) -> Value {
    view["fields"]
        .as_array()
        .expect("字段列表")
        .iter()
        .find(|field| field["key"] == json!(key))
        .unwrap_or_else(|| panic!("缺少配置项 {key}"))["value"]
        .clone()
}

async fn read_maintenance(app: &Router, token: &str) -> (Value, Value) {
    let (status, bytes) = call(
        app,
        request(Method::GET, "/admin/api/config", Some(token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "读配置应成功");
    let view = json_body(&bytes);
    (
        field_value(&view, "bot_maintenance"),
        field_value(&view, "bot_maintenance_message"),
    )
}

#[tokio::test]
async fn maintenance_switch_survives_a_restart() {
    let path = write_config("maintenance");
    let recorder = Recorder::new();
    let app = router(state(&path).with_config_listener(recorder.clone()));
    let token = login(&app).await;
    let (on, text) = read_maintenance(&app, &token).await;
    assert_eq!(on, json!(false), "初始状态应当关闭");
    assert_eq!(text, json!(""), "初始没有自定义文案");

    let body = json!({ "values": {
        "bot_maintenance": true,
        "bot_maintenance_message": "今晚维护, 稍后再来"
    }});
    let (status, bytes) = call(
        &app,
        request(Method::PUT, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "保存失败: {}",
        String::from_utf8_lossy(&bytes)
    );
    let saved = json_body(&bytes);
    assert_eq!(saved["values"]["bot_maintenance"], json!(true));
    assert_eq!(
        recorder.last().map(|config| config.bot_maintenance),
        Some(Some(true)),
        "保存后要通知运行时热更"
    );
    let (on, text) = read_maintenance(&app, &token).await;
    assert_eq!(on, json!(true), "保存后界面要立刻显示已开启");
    assert_eq!(text, json!("今晚维护, 稍后再来"));

    let stored = AppConfig::load(&path).expect("重新加载");
    assert_eq!(stored.bot_maintenance, Some(true));
    assert_eq!(stored.maintenance_message(), "今晚维护, 稍后再来");

    let restarted = router(state(&path));
    let token = login(&restarted).await;
    let (on, text) = read_maintenance(&restarted, &token).await;
    assert_eq!(on, json!(true), "重启后维护开关不能自己关掉");
    assert_eq!(text, json!("今晚维护, 稍后再来"));

    let body = json!({ "values": { "bot_maintenance": false } });
    let (status, _) = call(
        &restarted,
        request(Method::PUT, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let stored = AppConfig::load(&path).expect("重新加载");
    assert_eq!(stored.bot_maintenance, Some(false), "关掉也要落盘");
    let (on, _) = read_maintenance(&restarted, &token).await;
    assert_eq!(on, json!(false));
}

#[tokio::test]
async fn saving_creates_a_missing_config_file() {
    let dir = std::env::temp_dir().join(format!(
        "aurorix-admin-persist-missing-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("config.json");
    let config = AppConfig {
        appid: String::from("app-1"),
        secret: String::from("s"),
        admin_password: String::from(PASSWORD),
        ..AppConfig::default()
    };
    let app = router(
        AdminState::new(&path, config)
            .with_logger(Logger::new(Options::default().with_console(false))),
    );
    let token = login(&app).await;
    let body = json!({ "values": { "bot_maintenance": true } });
    let (status, bytes) = call(
        &app,
        request(Method::PUT, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "配置文件不存在时保存也要成功: {}",
        String::from_utf8_lossy(&bytes)
    );
    let stored = AppConfig::load(&path).expect("保存后应当有配置文件");
    assert_eq!(stored.bot_maintenance, Some(true));
    std::fs::remove_dir_all(&dir).ok();
}
