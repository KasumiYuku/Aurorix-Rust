use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use aurorix_admin::config::SECRET_MASK;
use aurorix_admin::{
    router, AdminState, MenuSource, Panel, PanelItem, PanelRecord, PluginError, PluginInfo,
    PluginSource,
};
use aurorix_db::AppConfig;
use aurorix_log::{Logger, Options};
use axum::body::Bytes;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Map, Value};
use tower::ServiceExt;

const PASSWORD: &str = "管理台口令-Test9";

const APP_SECRET: &str = "app-secret-值";

struct FixturePlugins {
    saved: Mutex<Vec<(String, Map<String, Value>)>>,
    error: Option<PluginError>,
}

impl FixturePlugins {
    fn new(error: Option<PluginError>) -> Self {
        Self {
            saved: Mutex::new(Vec::new()),
            error,
        }
    }

    fn saved(&self) -> Vec<(String, Map<String, Value>)> {
        self.saved.lock().expect("锁插件配置").clone()
    }
}

impl PluginSource for FixturePlugins {
    fn plugins(&self) -> Vec<PluginInfo> {
        vec![
            PluginInfo {
                id: "arcaea".to_string(),
                name: "Arcaea".to_string(),
                version: "1.0.0".to_string(),
                enabled: true,
                fields: vec![
                    json!({ "key": "theme", "label": "主题" }),
                    json!({ "key": "volume", "label": "音量" }),
                ],
                values: json!({ "theme": "dark", "volume": 60 }),
                ..PluginInfo::default()
            },
            PluginInfo {
                id: "phigros".to_string(),
                name: "Phigros".to_string(),
                version: "2.0.0".to_string(),
                enabled: false,
                ..PluginInfo::default()
            },
        ]
    }

    fn save_config(&self, id: &str, values: Map<String, Value>) -> Result<(), PluginError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if id != "arcaea" {
            return Err(PluginError::NotFound("插件不存在".to_string()));
        }
        self.saved
            .lock()
            .expect("锁插件配置")
            .push((id.to_string(), values));
        Ok(())
    }
}

struct FixtureMenu;

fn panel_record() -> PanelRecord {
    PanelRecord {
        panel_id: "p_g1".to_string(),
        scope: "group".to_string(),
        target_type: "all".to_string(),
        panel: Panel {
            items: vec![PanelItem {
                name: "签到".to_string(),
                desc: "每日签到".to_string(),
                kind: "command".to_string(),
                ..PanelItem::default()
            }],
            remark: String::new(),
            version: 1,
        },
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        version: 1,
        user_openids: Vec::new(),
        group_openids: Vec::new(),
    }
}

impl MenuSource for FixtureMenu {
    fn panels(&self, scope: &str) -> Result<Vec<PanelRecord>, String> {
        if scope == "group" {
            Ok(vec![panel_record()])
        } else {
            Ok(Vec::new())
        }
    }

    fn count(&self) -> Result<usize, String> {
        Ok(1)
    }

    fn get(&self, panel_id: &str) -> Option<PanelRecord> {
        if panel_id == "p_g1" {
            Some(panel_record())
        } else {
            None
        }
    }

    fn create(&self, scope: &str, _panel: Panel) -> Result<String, String> {
        Ok(format!("panel-{scope}"))
    }

    fn update(&self, _panel_id: &str, _panel: Panel) -> Result<i32, String> {
        Ok(2)
    }

    fn delete(&self, _panel_id: &str) -> Result<(), String> {
        Ok(())
    }
}

struct Fixture {
    state: AdminState,
    path: PathBuf,
    plugins: Arc<FixturePlugins>,
}

fn fixture(name: &str, plugin_error: Option<PluginError>) -> Fixture {
    let dir = std::env::temp_dir().join(format!("aurorix-admin-api-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("config.json");
    let raw = format!(
        r#"{{"port":8080,"appid":"app-1","secret":"{APP_SECRET}","admin_password":"{PASSWORD}"}}"#
    );
    std::fs::write(&path, raw).expect("写配置文件");
    let config = AppConfig::load(&path).expect("读配置文件");
    let plugins = Arc::new(FixturePlugins::new(plugin_error));
    let state = AdminState::new(&path, config)
        .with_plugins(plugins.clone())
        .with_menu(Arc::new(FixtureMenu))
        .with_logger(Logger::new(Options::default().with_console(false)));
    Fixture {
        state,
        path,
        plugins,
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

fn text_body(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

fn error_text(bytes: &[u8]) -> String {
    json_body(bytes)["error"]
        .as_str()
        .expect("错误响应需带 error 字段")
        .to_string()
}

fn find_field<'a>(fields: &'a [Value], key: &str) -> Option<&'a Value> {
    fields.iter().find(|field| field["key"] == json!(key))
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

fn read_disk(path: &PathBuf) -> Value {
    let raw = std::fs::read_to_string(path).expect("读配置");
    serde_json::from_str(&raw).expect("解析配置")
}

#[tokio::test]
async fn config_get_masks_secrets_and_returns_field_specs() {
    let fx = fixture("cfg-get", None);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/config", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "读取配置需 200");
    let view = json_body(&bytes);
    let fields = view["fields"].as_array().expect("需返回字段表");
    let secret = find_field(fields, "secret").expect("需返回 AppSecret 字段");
    assert_eq!(secret["value"], json!(""), "AppSecret 不得回显");
    assert_eq!(secret["set"], json!(true), "已设置的密钥需标记为已存在");
    assert_eq!(
        secret["kind"],
        json!("secret"),
        "AppSecret 需渲染成密钥输入框"
    );
    let password = find_field(fields, "admin_password").expect("需返回管理口令字段");
    assert_eq!(password["value"], json!(""), "管理口令不得回显");
    assert_eq!(
        password["set"],
        json!(true),
        "已设置的管理口令需标记为已存在"
    );
    let port = find_field(fields, "port").expect("需返回监听端口字段");
    assert_eq!(port["value"], json!(8080), "普通字段需原样返回");
    assert!(
        view["groups"]
            .as_array()
            .is_some_and(|groups| !groups.is_empty()),
        "需返回分组表"
    );
    let text = text_body(&bytes);
    assert!(!text.contains(APP_SECRET), "响应不得含 AppSecret 明文");
    assert!(!text.contains(PASSWORD), "响应不得含管理口令明文");
}

#[tokio::test]
async fn config_put_writes_writable_keys_and_reports_restart_required() {
    let fx = fixture("cfg-put", None);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "port": 9090, "log_level": "debug" } });
    let (status, bytes) = call(
        &app,
        request(Method::PUT, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "合法写入需 200");
    let result = json_body(&bytes);
    assert_eq!(result["ok"], json!(true), "写入成功需返回 ok");
    assert_eq!(
        result["restart_required"],
        json!(["port"]),
        "需要重启的键名需单独回给前端"
    );
    assert_eq!(result["restart_needed"], json!(true), "port 属重启字段");
    assert_eq!(
        result["hot_fields"],
        json!(["log_level"]),
        "log_level 属热更字段"
    );
    assert_eq!(
        result["values"]["port"],
        json!(9090),
        "响应里的 values 需体现写入结果"
    );
    assert_eq!(
        result["values"]["log_level"],
        json!("debug"),
        "响应里的 values 需体现写入结果"
    );
    let disk = read_disk(&fx.path);
    assert_eq!(disk["port"], json!(9090), "指定键需落盘");
    assert_eq!(disk["log_level"], json!("debug"), "指定键需落盘");
    assert_eq!(disk["appid"], json!("app-1"), "未指定键需原样保留");
    let (status, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/config", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "写入后读取需 200");
    let view = json_body(&bytes);
    let fields = view["fields"].as_array().expect("需返回字段表");
    let port = find_field(fields, "port").expect("需返回监听端口字段");
    assert_eq!(port["value"], json!(9090), "写入后需能读回");
}

#[tokio::test]
async fn config_put_skips_masked_secret_placeholder() {
    let fx = fixture("cfg-mask", None);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "secret": SECRET_MASK, "admin_password": "" } });
    let (status, bytes) = call(
        &app,
        request(Method::PUT, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "回传脱敏占位符需被忽略而不是报错");
    let result = json_body(&bytes);
    assert_eq!(
        result["restart_required"],
        json!([]),
        "被跳过的密钥不应出现在重启列表里"
    );
    assert_eq!(result["hot_fields"], json!([]), "被跳过的密钥不算热更");
    assert_eq!(
        fx.state.config_snapshot().secret,
        APP_SECRET,
        "脱敏占位符不得覆盖原密钥"
    );
    assert_eq!(fx.state.password(), PASSWORD, "空口令不得清空原口令");
    let disk = read_disk(&fx.path);
    assert_eq!(disk["secret"], json!(APP_SECRET), "落盘密钥需保持原值");
}

#[tokio::test]
async fn config_put_rejects_unknown_key_in_chinese() {
    let fx = fixture("cfg-unknown", None);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "unknown_field": 1 } });
    let (status, bytes) = call(
        &app,
        request(Method::PUT, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "未知键需 400");
    assert_eq!(
        error_text(&bytes),
        "不支持修改的配置项: unknown_field",
        "错误信息需为中文并指出键名"
    );
}

#[tokio::test]
async fn plugins_put_saves_one_known_plugin() {
    let fx = fixture("plugin-save", None);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "theme": "light", "volume": 30 });
    let (status, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/plugins/arcaea",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "已知插件写入需 200");
    assert_eq!(json_body(&bytes)["ok"], json!(true), "需返回 ok");
    let saved = fx.plugins.saved();
    assert_eq!(saved.len(), 1, "需写入一次插件配置");
    assert_eq!(saved[0].0, "arcaea", "写入的插件 id 需对得上");
    assert_eq!(
        saved[0].1.get("theme").and_then(Value::as_str),
        Some("light"),
        "写入的配置项需原样传给插件端口"
    );
}

#[tokio::test]
async fn plugins_put_reports_unknown_plugin_as_not_found() {
    let fx = fixture("plugin-missing", None);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "theme": "light" });
    let (status, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/plugins/not-a-plugin",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "未知插件需 404");
    assert_eq!(error_text(&bytes), "插件不存在", "错误信息需为中文");
    assert!(fx.plugins.saved().is_empty(), "未知插件不得写入任何配置");
}

#[tokio::test]
async fn plugins_put_rejects_unknown_field_with_key_name() {
    let fx = fixture("plugin-field", None);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "theme": "light", "glow": true });
    let (status, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/plugins/arcaea",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "未知配置项需 400");
    let message = error_text(&bytes);
    assert!(
        message.contains("glow"),
        "错误信息需带上未知键名, 实际为 {message}"
    );
    assert!(!message.is_ascii(), "错误信息需为中文");
    assert!(fx.plugins.saved().is_empty(), "校验失败不得写入插件配置");
}

#[tokio::test]
async fn plugins_put_reports_store_failure() {
    let fx = fixture(
        "plugin-fail",
        Some(PluginError::Server("磁盘只读".to_string())),
    );
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "theme": "light" });
    let (status, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/plugins/arcaea",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "插件端口失败需 500"
    );
    assert_eq!(error_text(&bytes), "保存配置失败", "错误信息需为中文");
}

#[tokio::test]
async fn menu_panels_routes_are_reachable() {
    let fx = fixture("menu-panels", None);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/menu/panels", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "面板列表需 200");
    let view = json_body(&bytes);
    let scopes = view["scopes"].as_array().expect("需返回场景列表");
    assert_eq!(scopes.len(), 4, "四个场景都要返回");
    assert_eq!(view["total"], json!(1), "需统计面板总数");
    let group = scopes
        .iter()
        .find(|scope| scope["scope"] == json!("group"))
        .expect("需包含群聊场景");
    let panels = group["panels"].as_array().expect("场景下需有面板数组");
    assert_eq!(panels.len(), 1, "群聊场景需返回一份面板");
    assert_eq!(panels[0]["panel_id"], json!("p_g1"), "需返回面板 id");
    assert_eq!(
        panels[0]["panel"]["items"][0]["name"],
        json!("签到"),
        "需返回面板条目"
    );
    let (status, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/menu/panels",
            Some(&token),
            Some(json!({
                "scope": "group",
                "panel": { "items": [{ "name": "帮助", "desc": "查看帮助", "type": "command" }] }
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "新建面板需 200");
    assert_eq!(
        json_body(&bytes)["panel_id"],
        json!("panel-group"),
        "新建需返回面板 id"
    );
    let (status, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/menu/panels/p_g1",
            Some(&token),
            Some(json!({ "panel": { "items": [] } })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "覆盖面板需 200");
    assert_eq!(json_body(&bytes)["version"], json!(2), "需返回新版本号");
}

#[tokio::test]
async fn menu_panels_reject_missing_ids() {
    let fx = fixture("menu-missing", None);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/menu/panels/p_nope",
            Some(&token),
            Some(json!({ "panel": { "items": [] } })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "覆盖不存在的面板需 404");
    assert_eq!(error_text(&bytes), "面板不存在", "错误信息需为中文");
    let (status, bytes) = call(
        &app,
        request(
            Method::DELETE,
            "/admin/api/menu/panels/p_nope",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "删除不存在的面板需 404");
    assert_eq!(error_text(&bytes), "面板不存在", "错误信息需为中文");
    let (status, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/menu/snapshots/nope.json/restore",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "回滚不存在的快照需 404");
    let message = error_text(&bytes);
    assert!(
        message.contains("快照不存在"),
        "错误信息需说明快照不存在, 实际为 {message}"
    );
}
