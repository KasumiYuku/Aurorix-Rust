use std::path::PathBuf;

use aurorix_admin::config::{field_views, is_hot, needs_restart, WRITABLE_KEYS};
use aurorix_admin::{router, AdminState};
use aurorix_db::AppConfig;
use aurorix_log::{Logger, Options};
use axum::body::Bytes;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

const PASSWORD: &str = "设置页口令-Test1";

const APP_SECRET: &str = "app-secret-字段";

const GROUP_IDS: [&str; 7] = [
    "maintenance",
    "connection",
    "console",
    "logging",
    "message",
    "footer",
    "member",
];

const FIELD_COUNT: usize = 36;

struct Fixture {
    state: AdminState,
    path: PathBuf,
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "aurorix-admin-fields-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("config.json");
    let raw = format!(
        r#"{{"port":8080,"appid":"app-1","secret":"{APP_SECRET}","admin_password":"{PASSWORD}","global_markdown":true,"footer_buttons":["帮助 | command | /help","+1 | repeat"]}}"#
    );
    std::fs::write(&path, raw).expect("写配置文件");
    let config = AppConfig::load(&path).expect("读配置文件");
    let state = AdminState::new(&path, config)
        .with_logger(Logger::new(Options::default().with_console(false)));
    Fixture { state, path }
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

async fn config_view(app: &Router, token: &str) -> Value {
    let (status, bytes) = call(
        app,
        request(Method::GET, "/admin/api/config", Some(token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "读取设置页需 200");
    json_body(&bytes)
}

fn fields_of(view: &Value) -> &Vec<Value> {
    view["fields"].as_array().expect("响应需带 fields 数组")
}

fn field_of<'a>(fields: &'a [Value], key: &str) -> &'a Value {
    fields
        .iter()
        .find(|field| field["key"] == json!(key))
        .unwrap_or_else(|| panic!("缺少字段 {key}"))
}

fn text_of(field: &Value, key: &str) -> String {
    field[key]
        .as_str()
        .unwrap_or_else(|| panic!("字段缺少字符串属性 {key}"))
        .to_string()
}

#[tokio::test]
async fn config_get_returns_seven_groups_and_thirty_six_fields() {
    let fx = fixture("shape");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let view = config_view(&app, &token).await;
    let groups = view["groups"].as_array().expect("响应需带 groups 数组");
    assert_eq!(groups.len(), 7, "分组数量需与 Go 版一致");
    let ids: Vec<String> = groups.iter().map(|group| text_of(group, "id")).collect();
    assert_eq!(ids, GROUP_IDS.to_vec(), "分组 ID 需与 Go 版一致且顺序固定");
    for group in groups {
        assert!(!text_of(group, "label").is_empty(), "分组需带中文标签");
        assert!(!text_of(group, "desc").is_empty(), "分组需带中文说明");
    }
    let fields = fields_of(&view);
    assert_eq!(fields.len(), FIELD_COUNT, "字段数量需与 Go 版一致");
    let mut seen = Vec::new();
    for field in fields {
        let key = text_of(field, "key");
        assert!(!key.is_empty(), "每个字段都要有 key");
        assert!(
            !text_of(field, "label").is_empty(),
            "字段 {key} 需带中文标签"
        );
        assert!(
            !text_of(field, "kind").is_empty(),
            "字段 {key} 需带控件类型"
        );
        let group = text_of(field, "group");
        assert!(
            GROUP_IDS.contains(&group.as_str()),
            "字段 {key} 的分组 {group} 不在已知分组里"
        );
        assert!(field.get("value").is_some(), "字段 {key} 需带 value");
        assert!(field["hot"].is_boolean(), "字段 {key} 需带 hot 标记");
        assert!(
            field["restart"].is_boolean(),
            "字段 {key} 需带 restart 标记"
        );
        assert!(field["set"].is_boolean(), "字段 {key} 需带 set 标记");
        if let Some(options) = field.get("options") {
            assert!(
                options.as_array().is_some_and(|list| !list.is_empty()),
                "字段 {key} 的 options 不得为空数组"
            );
        }
        assert!(
            WRITABLE_KEYS.contains(&key.as_str()),
            "字段 {key} 必须是可写配置项"
        );
        assert!(!seen.contains(&key), "字段 {key} 重复出现");
        seen.push(key);
    }
    assert_eq!(
        seen.len(),
        WRITABLE_KEYS.len(),
        "字段表需覆盖全部可写配置项"
    );
}

#[tokio::test]
async fn config_get_maps_values_from_app_config() {
    let fx = fixture("values");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let view = config_view(&app, &token).await;
    let fields = fields_of(&view);
    assert_eq!(
        field_of(fields, "port")["value"],
        json!(8080),
        "端口需取配置值"
    );
    assert_eq!(
        field_of(fields, "global_markdown")["value"],
        json!(true),
        "全局 Markdown 开关需取配置值"
    );
    assert_eq!(
        field_of(fields, "footer_buttons")["value"],
        json!(["帮助 | command | /help", "+1 | repeat"]),
        "底部按钮需按行返回"
    );
    assert_eq!(
        field_of(fields, "prefixes")["value"],
        json!(["/", "#", "无前缀"]),
        "空前缀需显示成「无前缀」"
    );
    assert_eq!(
        field_of(fields, "protocol")["value"],
        json!("webhook"),
        "协议需取规范化后的默认值"
    );
    assert_eq!(
        field_of(fields, "appid")["value"],
        json!("app-1"),
        "AppID 需取配置值"
    );
    assert_eq!(
        field_of(fields, "image_max_width")["value"],
        json!(350),
        "图片宽度需取默认值"
    );
    assert_eq!(
        field_of(fields, "bot_maintenance")["value"],
        json!(false),
        "维护开关默认关闭"
    );
    let intents = field_of(fields, "intents")["value"]
        .as_array()
        .expect("订阅事件需为数组");
    assert_eq!(intents.len(), 13, "订阅事件需取默认订阅");
    assert_eq!(
        field_of(fields, "intents")["options"]
            .as_array()
            .expect("订阅事件需带可选项")
            .len(),
        28,
        "订阅事件可选全集需与 Go 版一致"
    );
}

#[tokio::test]
async fn config_get_keeps_secret_value_empty_and_marks_set() {
    let fx = fixture("secrets");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let view = config_view(&app, &token).await;
    let fields = fields_of(&view);
    let secret = field_of(fields, "secret");
    assert_eq!(secret["value"], json!(""), "AppSecret 的值需为空串");
    assert_eq!(secret["set"], json!(true), "已配置的 AppSecret 需标记 set");
    assert_eq!(secret["kind"], json!("secret"), "AppSecret 需渲染成密钥框");
    assert_eq!(secret["restart"], json!(true), "AppSecret 属重启字段");
    let password = field_of(fields, "admin_password");
    assert_eq!(password["value"], json!(""), "管理密码的值需为空串");
    assert_eq!(password["set"], json!(true), "已配置的管理密码需标记 set");
    assert_eq!(password["hot"], json!(true), "管理密码属热更字段");
    let empty = AppConfig::default();
    let views = field_views(&empty);
    let secret = field_of(&views, "secret");
    assert_eq!(
        secret["set"],
        json!(false),
        "未配置的 AppSecret 需标记未设置"
    );
    assert_eq!(
        field_of(&views, "admin_password")["set"],
        json!(false),
        "未配置的管理密码需标记未设置"
    );
}

#[tokio::test]
async fn config_put_writes_only_submitted_keys() {
    let fx = fixture("sparse");
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "values": { "log_level": "debug" } });
    let (status, bytes) = call(
        &app,
        request(Method::PUT, "/admin/api/config", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "稀疏补丁需 200");
    let result = json_body(&bytes);
    assert_eq!(result["ok"], json!(true), "写入成功需返回 ok");
    assert_eq!(
        result["restart_needed"],
        json!(false),
        "log_level 不需要重启"
    );
    assert_eq!(result["restart_fields"], json!([]), "不应有重启字段");
    assert_eq!(
        result["hot_fields"],
        json!(["log_level"]),
        "log_level 属热更字段"
    );
    let disk: Value = serde_json::from_str(&std::fs::read_to_string(&fx.path).expect("读配置"))
        .expect("解析配置");
    assert_eq!(disk["log_level"], json!("debug"), "提交的键需落盘");
    assert_eq!(disk["port"], json!(8080), "未提交的端口需保持原值");
    assert_eq!(disk["appid"], json!("app-1"), "未提交的 AppID 需保持原值");
    assert_eq!(
        disk["footer_buttons"],
        json!(["帮助 | command | /help", "+1 | repeat"]),
        "未提交的底部按钮需保持原值"
    );
    let view = config_view(&app, &token).await;
    assert_eq!(
        field_of(fields_of(&view), "log_level")["value"],
        json!("debug"),
        "写入后字段值需更新"
    );
    assert_eq!(
        field_of(fields_of(&view), "port")["value"],
        json!(8080),
        "未提交的字段值不得变化"
    );
}

#[test]
fn spec_flags_match_writable_and_restart_tables() {
    let views = field_views(&AppConfig::default());
    for field in &views {
        let key = text_of(field, "key");
        assert_eq!(
            field["restart"],
            json!(needs_restart(&key)),
            "字段 {key} 的 restart 标记需与重启键表一致"
        );
        assert_eq!(
            field["hot"],
            json!(is_hot(&key)),
            "字段 {key} 的 hot 标记需与字段表一致"
        );
        assert_eq!(
            field["restart"],
            json!(!field["hot"].as_bool().unwrap_or(false)),
            "字段 {key} 必须要么热更要么需重启"
        );
    }
    assert_eq!(
        views
            .iter()
            .filter(|field| field["restart"] == json!(true))
            .count(),
        11,
        "需要重启的字段数量需与 Go 版一致"
    );
}
