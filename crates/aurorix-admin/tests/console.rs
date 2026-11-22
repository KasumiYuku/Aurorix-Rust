use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_admin::{
    router, AdminState, AssetSource, AssetsView, BanEntry, BanSource, Control, Conversation,
    InboxMessage, InboxSendError, InboxSource, JobInfo, JobSource, MenuSnapshot, MenuSource, Panel,
    PanelRecord, PluginError, PluginInfo, PluginSource, PluginWebUi, ProfileSource, WebUiSource,
    UNSUPERVISED_RESTART_ERROR,
};
use aurorix_db::AppConfig;
use axum::body::Bytes;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

const PASSWORD: &str = "sup3r-secret-pwd";

struct MockMenu {
    records: Mutex<Vec<PanelRecord>>,
    snapshots: Mutex<Vec<MenuSnapshot>>,
}

impl MockMenu {
    fn with_panel(scope: &str, panel_id: &str) -> Self {
        Self {
            records: Mutex::new(vec![PanelRecord {
                panel_id: panel_id.to_string(),
                scope: scope.to_string(),
                target_type: "all".to_string(),
                panel: Panel {
                    items: vec![aurorix_admin::PanelItem {
                        name: "查询".to_string(),
                        desc: "查分".to_string(),
                        kind: "command".to_string(),
                        only_admin: false,
                        link: String::new(),
                    }],
                    remark: "旧面板".to_string(),
                    version: 1,
                },
                created_at: "2024-01-01T00:00:00Z".to_string(),
                updated_at: "2024-01-01T00:00:00Z".to_string(),
                version: 1,
                user_openids: Vec::new(),
                group_openids: Vec::new(),
            }]),
            snapshots: Mutex::new(Vec::new()),
        }
    }
}

impl MenuSource for MockMenu {
    fn panels(&self, scope: &str) -> Result<Vec<PanelRecord>, String> {
        let records = self.records.lock().expect("锁面板表");
        Ok(records
            .iter()
            .filter(|record| record.scope == scope)
            .cloned()
            .collect())
    }

    fn count(&self) -> Result<usize, String> {
        Ok(self.records.lock().expect("锁面板表").len())
    }

    fn get(&self, panel_id: &str) -> Option<PanelRecord> {
        self.records
            .lock()
            .expect("锁面板表")
            .iter()
            .find(|record| record.panel_id == panel_id)
            .cloned()
    }

    fn create(&self, scope: &str, panel: Panel) -> Result<String, String> {
        let mut records = self.records.lock().expect("锁面板表");
        let panel_id = format!("p{}", records.len() + 1);
        records.push(PanelRecord {
            panel_id: panel_id.clone(),
            scope: scope.to_string(),
            target_type: "all".to_string(),
            panel,
            created_at: "2024-01-02T00:00:00Z".to_string(),
            updated_at: "2024-01-02T00:00:00Z".to_string(),
            version: 1,
            user_openids: Vec::new(),
            group_openids: Vec::new(),
        });
        Ok(panel_id)
    }

    fn update(&self, panel_id: &str, panel: Panel) -> Result<i32, String> {
        let mut records = self.records.lock().expect("锁面板表");
        for record in records.iter_mut() {
            if record.panel_id == panel_id {
                record.version += 1;
                record.panel = panel;
                return Ok(record.version);
            }
        }
        Err("面板不存在".to_string())
    }

    fn delete(&self, panel_id: &str) -> Result<(), String> {
        let mut records = self.records.lock().expect("锁面板表");
        records.retain(|record| record.panel_id != panel_id);
        Ok(())
    }

    fn snapshots(&self) -> Vec<MenuSnapshot> {
        self.snapshots.lock().expect("锁快照").clone()
    }

    fn save_snapshot(&self, action: &str, record: &PanelRecord) -> Result<String, String> {
        let file = format!("20240102-150405-{}.json", record.panel_id);
        self.snapshots.lock().expect("锁快照").push(MenuSnapshot {
            file: file.clone(),
            panel_id: record.panel_id.clone(),
            scope: record.scope.clone(),
            target_type: record.target_type.clone(),
            panel: record.panel.clone(),
            version: record.version,
            action: action.to_string(),
            saved_at: "2024-01-02T15:04:05Z".to_string(),
        });
        Ok(file)
    }

    fn read_snapshot(&self, name: &str) -> Option<MenuSnapshot> {
        self.snapshots
            .lock()
            .expect("锁快照")
            .iter()
            .find(|snapshot| snapshot.file == name)
            .cloned()
    }
}

struct MockBans {
    entries: Mutex<Vec<BanEntry>>,
}

impl MockBans {
    fn new() -> Self {
        Self {
            entries: Mutex::new(vec![BanEntry {
                openid: "user-1".to_string(),
                message: "别再来了".to_string(),
                note: "刷屏".to_string(),
                at: "2024-01-01T00:00:00Z".to_string(),
            }]),
        }
    }
}

impl BanSource for MockBans {
    fn list(&self) -> Vec<BanEntry> {
        self.entries.lock().expect("锁封禁名单").clone()
    }

    fn put(&self, entry: BanEntry) -> Result<(), String> {
        self.entries.lock().expect("锁封禁名单").push(entry);
        Ok(())
    }

    fn delete(&self, openid: &str) -> Result<(), String> {
        let mut entries = self.entries.lock().expect("锁封禁名单");
        let before = entries.len();
        entries.retain(|entry| entry.openid != openid);
        if entries.len() == before {
            return Err("该用户不在封禁名单里".to_string());
        }
        Ok(())
    }
}

struct MockInbox;

impl InboxSource for MockInbox {
    fn conversations(&self) -> Result<Vec<Conversation>, String> {
        Ok(vec![Conversation {
            id: "group-1".to_string(),
            kind: "group".to_string(),
            name: "测试群".to_string(),
            last_at: "2024-01-01T00:00:00Z".to_string(),
            last_preview: "你好".to_string(),
        }])
    }

    fn messages(
        &self,
        conv_id: &str,
        _before: Option<i64>,
        limit: usize,
    ) -> Result<Vec<InboxMessage>, String> {
        Ok((0..2)
            .take(limit)
            .map(|index| InboxMessage {
                id: index as i64 + 1,
                conv_id: conv_id.to_string(),
                kind: "group".to_string(),
                direction: if index == 0 {
                    "in".to_string()
                } else {
                    "out".to_string()
                },
                sender_id: "user-1".to_string(),
                sender_name: "小明".to_string(),
                content: format!("第 {index} 条"),
                platform_id: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .collect())
    }

    fn lookup(&self, conv_id: &str) -> Option<Conversation> {
        if conv_id == "group-1" {
            return Some(Conversation {
                id: conv_id.to_string(),
                kind: "group".to_string(),
                name: "测试群".to_string(),
                last_at: "2024-01-01T00:00:00Z".to_string(),
                last_preview: "你好".to_string(),
            });
        }
        None
    }

    fn send(&self, conv: &Conversation, text: &str) -> Result<(), InboxSendError> {
        if conv.kind == "group" && !text.is_empty() {
            return Ok(());
        }
        Err(InboxSendError::Failed("发送失败".to_string()))
    }
}

struct MockJobs {
    paused: Mutex<Vec<String>>,
}

impl JobSource for MockJobs {
    fn jobs(&self) -> Vec<JobInfo> {
        let paused = self.paused.lock().expect("锁任务表");
        vec![JobInfo {
            id: "daily-report".to_string(),
            plugin_id: "arcaea".to_string(),
            kind: "cron".to_string(),
            cron: "0 8 * * *".to_string(),
            interval_ms: 0,
            immediate: false,
            paused: paused.iter().any(|id| id == "daily-report"),
            last_fire: 1700000000000,
            next_fire: 1700003600000,
        }]
    }

    fn exists(&self, id: &str) -> bool {
        id == "daily-report"
    }

    fn set_paused(&self, id: &str, paused: bool) {
        let mut state = self.paused.lock().expect("锁任务表");
        state.retain(|item| item != id);
        if paused {
            state.push(id.to_string());
        }
    }
}

struct MockAssets;

impl AssetSource for MockAssets {
    fn view(&self) -> AssetsView {
        AssetsView {
            whitelist: vec!["cdn.example.com".to_string()],
            providers: vec![aurorix_admin::AssetProvider {
                name: "local".to_string(),
                enabled: true,
                priority: 10,
                configured: true,
                has_secrets: true,
                schema: vec![aurorix_admin::AssetField {
                    key: "token".to_string(),
                    label: "密钥".to_string(),
                    description: String::new(),
                    kind: "password".to_string(),
                    required: true,
                    placeholder: String::new(),
                    default: None,
                    options: Vec::new(),
                }],
                config: serde_json::Map::new(),
            }],
        }
    }
}

struct MockPlugins {
    saved: Mutex<Vec<String>>,
}

impl MockPlugins {
    fn new() -> Self {
        Self {
            saved: Mutex::new(Vec::new()),
        }
    }
}

impl PluginSource for MockPlugins {
    fn plugins(&self) -> Vec<PluginInfo> {
        vec![PluginInfo {
            id: "arcaea".to_string(),
            name: "Arcaea".to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
            commands: vec![String::from("a")],
            ..PluginInfo::default()
        }]
    }

    fn save_config(
        &self,
        id: &str,
        values: serde_json::Map<String, Value>,
    ) -> Result<(), PluginError> {
        if id != "arcaea" {
            return Err(PluginError::BadRequest(format!(
                "plugin {id} is not registered"
            )));
        }
        self.saved
            .lock()
            .expect("锁插件配置")
            .push(format!("config:{}", values.len()));
        Ok(())
    }

    fn save_access(&self, id: &str, _access: Value) -> Result<(), PluginError> {
        if id != "arcaea" {
            return Err(PluginError::BadRequest(format!(
                "plugin {id} is not registered"
            )));
        }
        self.saved
            .lock()
            .expect("锁插件配置")
            .push("access".to_string());
        Ok(())
    }
}

struct MockProfile {
    available: bool,
}

impl ProfileSource for MockProfile {
    fn available(&self) -> bool {
        self.available
    }

    fn get(&self) -> Option<Value> {
        if self.available {
            return Some(json!({ "bot_name": "小助手" }));
        }
        None
    }

    fn refresh(&self) -> Result<Value, String> {
        Ok(json!({ "bot_name": "刷新后的小助手" }))
    }
}

struct MockWebUi {
    root: PathBuf,
}

async fn plugin_ping() -> axum::Json<Value> {
    axum::Json(json!({ "ok": true, "plugin": "arcaea" }))
}

impl WebUiSource for MockWebUi {
    fn web_uis(&self) -> Vec<PluginWebUi> {
        let api = Router::new().route("/ping", get(plugin_ping));
        vec![PluginWebUi::new("arcaea")
            .with_api(api)
            .with_spa_root(self.root.clone())]
    }
}

struct Fixture {
    state: AdminState,
    menu: Arc<MockMenu>,
    jobs: Arc<MockJobs>,
    plugins: Arc<MockPlugins>,
    restarts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

fn fixture(name: &str, supervised: bool, profile_available: bool) -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "aurorix-admin-console-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let spa = dir.join("spa");
    std::fs::create_dir_all(spa.join("assets")).expect("建前端目录");
    std::fs::write(spa.join("index.html"), "<!doctype html>index").expect("写 index");
    std::fs::write(spa.join("assets/app.js"), "console.log(1)").expect("写静态资源");
    let path = dir.join("config.json");
    let raw = format!(r#"{{"port":8080,"admin_password":"{PASSWORD}"}}"#);
    std::fs::write(&path, raw).expect("写配置文件");
    let config = AppConfig::load(&path).expect("读配置文件");
    let menu = Arc::new(MockMenu::with_panel("group", "p_g1"));
    let jobs = Arc::new(MockJobs {
        paused: Mutex::new(Vec::new()),
    });
    let plugins = Arc::new(MockPlugins::new());
    let restarts = Arc::new(AtomicUsize::new(0));
    let stops = Arc::new(AtomicUsize::new(0));
    let restart_counter = restarts.clone();
    let stop_counter = stops.clone();
    let control = Control {
        supervised,
        restart: Some(Arc::new(move || {
            restart_counter.fetch_add(1, Ordering::SeqCst);
        })),
        stop: Some(Arc::new(move || {
            stop_counter.fetch_add(1, Ordering::SeqCst);
        })),
    };
    let state = AdminState::new(&path, config)
        .with_menu(menu.clone())
        .with_bans(Arc::new(MockBans::new()))
        .with_inbox(Arc::new(MockInbox))
        .with_jobs(jobs.clone())
        .with_plugins(plugins.clone())
        .with_assets(Arc::new(MockAssets))
        .with_profile(Arc::new(MockProfile {
            available: profile_available,
        }))
        .with_webui(Arc::new(MockWebUi { root: spa }))
        .with_control(control);
    Fixture {
        state,
        menu,
        jobs,
        plugins,
        restarts,
        stops,
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

fn raw_request(method: Method, uri: &str, token: &str, body: &str) -> Request<Bytes> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Bytes::from(body.to_string()))
        .expect("构造请求")
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

fn header_value(headers: &HeaderMap, name: header::HeaderName) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

async fn login(app: &Router) -> String {
    let body = json!({ "password": PASSWORD });
    let (status, _, bytes) = call(
        app,
        request(Method::POST, "/admin/api/login", None, Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "登录需成功");
    json_body(&bytes)["token"]
        .as_str()
        .expect("需返回 token")
        .to_string()
}

#[tokio::test]
async fn menu_meta_endpoint_matches_go_limits() {
    let fx = fixture("menu-meta", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/menu/meta", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(
        value["scopes"],
        json!([
            { "scope": "group", "label": "群聊" },
            { "scope": "c2c", "label": "私聊" },
            { "scope": "channel", "label": "频道" },
            { "scope": "dm", "label": "频道私信" },
        ]),
        "场景顺序与中文名需与 Go 一致"
    );
    assert_eq!(value["limits"]["name_width"], json!(14));
    assert_eq!(value["limits"]["desc_width"], json!(30));
    assert_eq!(value["limits"]["remark_len"], json!(255));
    assert_eq!(value["limits"]["max_items"], json!(20));
    assert_eq!(value["limits"]["max_per_bot"], json!(20));
    assert_eq!(value["limits"]["item_types"], json!(["command", "link"]));
    assert_eq!(value["limits"]["target_type"], json!("all"));
    assert_eq!(
        value["limits"]["display_note"],
        json!("长度按显示宽度算: 中文与全角标点各计 2")
    );
}

#[tokio::test]
async fn menu_panels_list_groups_by_scope() {
    let fx = fixture("menu-list", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/menu/panels", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["total"], json!(1), "需统计四场景合计");
    assert_eq!(value["max_per_bot"], json!(20));
    let scopes = value["scopes"].as_array().expect("需返回 scopes");
    assert_eq!(scopes.len(), 4, "四个场景各一项");
    assert_eq!(scopes[0]["scope"], json!("group"));
    assert_eq!(scopes[0]["panels"][0]["panel_id"], json!("p_g1"));
    assert_eq!(scopes[0]["panels"][0]["target_type"], json!("all"));
    assert_eq!(
        scopes[0]["panels"][0]["panel"]["items"][0]["type"],
        json!("command")
    );
    assert_eq!(scopes[1]["panels"], json!([]), "空场景需返回空数组");
}

#[tokio::test]
async fn menu_create_panel_returns_new_id() {
    let fx = fixture("menu-create", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({
        "scope": "dm",
        "panel": { "remark": "新面板", "items": [ { "name": "a", "desc": "x", "type": "command" } ] }
    });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/menu/panels",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["ok"], json!(true));
    assert_eq!(value["panel_id"], json!("p2"), "需返回新建的面板 id");
    assert_eq!(fx.menu.count().expect("统计"), 2, "面板需真的落库");
}

#[tokio::test]
async fn menu_create_rejects_unknown_scope_in_chinese() {
    let fx = fixture("menu-create-bad", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "scope": "guild", "panel": { "items": [] } });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/menu/panels",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_body(&bytes)["error"], json!("场景 \"guild\" 不认识"));
    let (status, _, bytes) = call(
        &app,
        raw_request(
            Method::POST,
            "/admin/api/menu/panels",
            &token,
            "{ this is not json",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "非法 JSON 需 400");
    assert_eq!(json_body(&bytes)["error"], json!("请求格式无效"));
}

#[tokio::test]
async fn menu_create_blocks_when_panel_limit_reached() {
    let fx = fixture("menu-limit", false, false);
    {
        let mut records = fx.menu.records.lock().expect("锁面板表");
        while records.len() < 20 {
            let index = records.len();
            records.push(PanelRecord {
                panel_id: format!("p{index}"),
                scope: "c2c".to_string(),
                target_type: "all".to_string(),
                ..PanelRecord::default()
            });
        }
    }
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "scope": "channel", "panel": { "items": [] } });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/menu/panels",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "满 20 需 409");
    let message = json_body(&bytes)["error"]
        .as_str()
        .expect("需返回中文错误")
        .to_string();
    assert!(message.contains("四个场景合计"), "需说明上限跨场景合计");
}

#[tokio::test]
async fn menu_update_panel_bumps_version_and_snapshots() {
    let fx = fixture("menu-update", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({
        "panel": { "remark": "改过", "items": [ { "name": "b", "desc": "y", "type": "command" } ] }
    });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/menu/panels/p_g1",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["ok"], json!(true));
    assert_eq!(value["version"], json!(2), "更新后版本需自增");
    assert_eq!(value["snapshot"], json!("20240102-150405-p_g1.json"));
    assert_eq!(fx.menu.snapshots().len(), 1, "更新前需留快照");
}

#[tokio::test]
async fn menu_update_rejects_bad_panel_and_missing_id() {
    let fx = fixture("menu-update-bad", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({
        "panel": { "items": [ { "name": "名字太长太长太长太长太长太长了", "desc": "x", "type": "command" } ] }
    });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/menu/panels/p_g1",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "超宽名字需 400");
    assert!(json_body(&bytes)["error"]
        .as_str()
        .expect("中文错误")
        .contains("超过上限 14"));
    let body = json!({ "panel": { "items": [] } });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/menu/panels/p_nope",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "未知面板需 404");
    assert_eq!(json_body(&bytes)["error"], json!("面板不存在"));
}

#[tokio::test]
async fn menu_delete_panel_removes_record() {
    let fx = fixture("menu-delete", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(
            Method::DELETE,
            "/admin/api/menu/panels/p_g1",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["ok"], json!(true));
    assert_eq!(fx.menu.count().expect("统计"), 0, "面板需被删除");
    let (status, _, _) = call(
        &app,
        request(
            Method::DELETE,
            "/admin/api/menu/panels/p_g1",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "重复删除需 404");
}

#[tokio::test]
async fn menu_sync_copies_items_to_targets() {
    let fx = fixture("menu-sync", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({
        "from": "p_g1",
        "targets": [ { "scope": "c2c", "panel_id": "" } ]
    });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/menu/sync",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["ok"], json!(true));
    assert_eq!(value["from"], json!("p_g1"));
    assert_eq!(value["results"][0]["mode"], json!("created"));
    assert_eq!(value["results"][0]["scope"], json!("c2c"));
    assert_eq!(
        value["results"][0]["panel_id"],
        json!("p2"),
        "新建目标需回传新 id"
    );
    let body = json!({ "from": "", "targets": [] });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/menu/sync",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_body(&bytes)["error"], json!("没说要同步哪一份面板"));
}

#[tokio::test]
async fn menu_snapshots_list_then_restore() {
    let fx = fixture("menu-snapshots", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({
        "panel": { "remark": "改过", "items": [ { "name": "b", "desc": "y", "type": "command" } ] }
    });
    let (status, _, _) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/menu/panels/p_g1",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/menu/snapshots", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["snapshots"][0]["action"], json!("update"));
    assert_eq!(value["snapshots"][0]["panel_id"], json!("p_g1"));
    assert_eq!(value["dir"], json!("data/menu-snapshots"));
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/menu/snapshots/20240102-150405-p_g1.json/restore",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["mode"], json!("overwrite"));
    assert_eq!(value["panel_id"], json!("p_g1"));
    assert_eq!(value["scope"], json!("group"));
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/menu/snapshots/..%2Fetc%2Fpasswd/restore",
            Some(&token),
            None,
        ),
    )
    .await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
        "非法快照名需被挡: {status}"
    );
    assert!(!json_body(&bytes)["error"]
        .as_str()
        .expect("中文错误")
        .is_empty());
}

#[tokio::test]
async fn menu_routes_require_session() {
    let fx = fixture("menu-auth", false, false);
    let app = router(fx.state.clone());
    for uri in [
        "/admin/api/menu/meta",
        "/admin/api/menu/panels",
        "/admin/api/menu/snapshots",
    ] {
        let (status, _, bytes) = call(&app, request(Method::GET, uri, None, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri} 未登录需 401");
        assert_eq!(json_body(&bytes)["error"], json!("未登录或会话已失效"));
    }
}

#[tokio::test]
async fn bans_list_put_and_delete_roundtrip() {
    let fx = fixture("bans", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/bans", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["entries"][0]["openid"], json!("user-1"));
    assert_eq!(value["entries"][0]["note"], json!("刷屏"));
    assert_eq!(value["default"], json!("您的账号因违反Bot使用协议已被封禁"));
    assert_eq!(value["limit"], json!(2000));
    let body = json!({ "openid": "user-2", "message": "冷静一下", "note": "广告" });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/bans", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["ok"], json!(true));
    assert_eq!(value["entries"].as_array().map(Vec::len), Some(2));
    let (status, _, bytes) = call(
        &app,
        request(Method::DELETE, "/admin/api/bans/user-1", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json_body(&bytes)["entries"].as_array().map(Vec::len),
        Some(1)
    );
}

#[tokio::test]
async fn bans_reject_blank_openid_and_unknown_delete() {
    let fx = fixture("bans-bad", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "openid": "   ", "message": "x" });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/bans", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_body(&bytes)["error"], json!("openid 不能为空"));
    let body = json!({ "openid": "a b", "message": "x" });
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/bans", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_body(&bytes)["error"], json!("openid 不能包含空白字符"));
    let (status, _, bytes) = call(
        &app,
        request(
            Method::DELETE,
            "/admin/api/bans/user-404",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_body(&bytes)["error"], json!("该用户不在封禁名单里"));
    let (status, _, _) = call(&app, request(Method::GET, "/admin/api/bans", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "封禁名单未登录需 401");
}

#[tokio::test]
async fn inbox_conversations_and_messages_list() {
    let fx = fixture("inbox", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(
            Method::GET,
            "/admin/api/inbox/conversations",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["conversations"][0]["id"], json!("group-1"));
    assert_eq!(value["conversations"][0]["kind"], json!("group"));
    assert_eq!(value["conversations"][0]["last_preview"], json!("你好"));
    let (status, _, bytes) = call(
        &app,
        request(
            Method::GET,
            "/admin/api/inbox/messages?conv=group-1&limit=2",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(value["messages"][0]["direction"], json!("in"));
    assert_eq!(value["messages"][0]["content"], json!("第 0 条"));
    assert_eq!(value["messages"][0]["conv_id"], json!("group-1"));
    assert_eq!(value["has_more"], json!(true), "取满一页需标记还有更多");
    let (status, _, bytes) = call(
        &app,
        request(
            Method::GET,
            "/admin/api/inbox/messages?conv=",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_body(&bytes)["error"], json!("缺少会话 id"));
}

#[tokio::test]
async fn inbox_send_accepts_known_conversation() {
    let fx = fixture("inbox-send", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "conv": "group-1", "text": " 你好 " });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/inbox/messages",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["ok"], json!(true));
    let body = json!({ "conv": "group-404", "text": "你好" });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/inbox/messages",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json_body(&bytes)["error"], json!("会话不存在"));
    let body = json!({ "conv": "group-1", "text": "   " });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/inbox/messages",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_body(&bytes)["error"], json!("会话和内容都不能为空"));
}

#[tokio::test]
async fn jobs_list_returns_bare_array_like_go() {
    let fx = fixture("jobs", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/jobs", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    let jobs = value.as_array().expect("Go 原版直接返回数组");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["id"], json!("daily-report"));
    assert_eq!(jobs[0]["plugin_id"], json!("arcaea"));
    assert_eq!(jobs[0]["kind"], json!("cron"));
    assert_eq!(jobs[0]["cron"], json!("0 8 * * *"));
    assert_eq!(jobs[0]["paused"], json!(false));
    assert_eq!(jobs[0]["last_fire"], json!(1700000000000i64));
    assert_eq!(jobs[0]["next_fire"], json!(1700003600000i64));
}

#[tokio::test]
async fn job_pause_toggles_and_rejects_unknown_id() {
    let fx = fixture("jobs-pause", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let body = json!({ "paused": true });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/jobs/daily-report/pause",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["ok"], json!(true));
    assert_eq!(
        fx.jobs.paused.lock().expect("锁任务表").len(),
        1,
        "暂停状态需落到任务源"
    );
    let body = json!({ "paused": false });
    let (status, _, _) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/jobs/nope/pause",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "未知任务需 404");
    let (status, _, bytes) = call(
        &app,
        raw_request(
            Method::POST,
            "/admin/api/jobs/daily-report/pause",
            &token,
            "not json",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json_body(&bytes)["error"], json!("请求格式无效"));
}

#[tokio::test]
async fn assets_get_and_put_roundtrip() {
    let fx = fixture("assets", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/assets", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value = json_body(&bytes);
    assert_eq!(value["whitelist"], json!(["cdn.example.com"]));
    assert_eq!(value["providers"][0]["name"], json!("local"));
    assert_eq!(value["providers"][0]["configured"], json!(true));
    assert_eq!(value["providers"][0]["has_secrets"], json!(true));
    assert_eq!(value["providers"][0]["schema"][0]["key"], json!("token"));
    assert_eq!(
        value["providers"][0]["schema"][0]["type"],
        json!("password")
    );
    assert_eq!(value["providers"][0]["schema"][0]["required"], json!(true));
    let body = json!({
        "whitelist": ["cdn.example.com"],
        "providers": [ { "name": "local", "enabled": true, "priority": 10, "config": { "token": "" } } ]
    });
    let (status, _, bytes) = call(
        &app,
        request(Method::PUT, "/admin/api/assets", Some(&token), Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["ok"], json!(true));
    let (status, _, _) = call(&app, request(Method::GET, "/admin/api/assets", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "资产接口未登录需 401");
}

#[tokio::test]
async fn profile_get_and_refresh_when_available() {
    let fx = fixture("profile", false, true);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/profile", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["profile"]["bot_name"], json!("小助手"));
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/profile/refresh",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json_body(&bytes)["profile"]["bot_name"],
        json!("刷新后的小助手")
    );
}

#[tokio::test]
async fn profile_refresh_returns_503_when_unavailable() {
    let fx = fixture("profile-off", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/profile", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["profile"], json!(null));
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/profile/refresh",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json_body(&bytes)["error"], json!("档案服务未启用"));
}

#[tokio::test]
async fn system_restart_blocks_without_supervisor() {
    let fx = fixture("restart-blocked", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/system/restart",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "未守护需 409");
    assert_eq!(
        json_body(&bytes)["error"],
        json!(UNSUPERVISED_RESTART_ERROR),
        "文案需与 Go 逐字一致"
    );
    assert_eq!(
        json_body(&bytes)["error"],
        json!("未声明受外部守护(AURORIX_SUPERVISED=1), 面板重启会与守护进程各拉一个实例互杀; 请手动重启或在受管模式下使用")
    );
    assert_eq!(
        fx.restarts.load(Ordering::SeqCst),
        0,
        "被拦下时不得触发回调"
    );
}

#[tokio::test]
async fn system_restart_and_stop_trigger_callbacks() {
    let fx = fixture("restart-ok", true, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(
            Method::POST,
            "/admin/api/system/restart",
            Some(&token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let value = json_body(&bytes);
    assert_eq!(value["ok"], json!(true));
    assert_eq!(value["notice"], json!("重启已触发"));
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/system/stop", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(json_body(&bytes)["notice"], json!("停止已触发"));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(fx.restarts.load(Ordering::SeqCst), 1, "重启回调需被触发");
    assert_eq!(fx.stops.load(Ordering::SeqCst), 1, "停止回调需被触发");
}

#[tokio::test]
async fn system_control_reports_not_implemented_without_callback() {
    let fx = fixture("control-off", true, false);
    let control = Control {
        supervised: true,
        restart: None,
        stop: None,
    };
    let state = fx.state.clone().with_control(control);
    let app = router(state);
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::POST, "/admin/api/system/stop", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(json_body(&bytes)["error"], json!("停止控制未启用"));
}

#[tokio::test]
async fn plugin_api_uses_admin_session() {
    let fx = fixture("plugin-api", true, false);
    let app = router(fx.state.clone());
    let (status, _, bytes) = call(&app, request(Method::GET, "/arcaea/api/ping", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "插件 API 未登录需 401");
    assert_eq!(json_body(&bytes)["error"], json!("未登录或会话已失效"));
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/arcaea/api/ping", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["plugin"], json!("arcaea"));
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/arcaea/api/none", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json_body(&bytes)["error"], json!("接口不存在"));
}

#[tokio::test]
async fn plugin_spa_serves_assets_with_immutable_cache() {
    let fx = fixture("spa-assets", false, false);
    let app = router(fx.state.clone());
    let (status, headers, bytes) = call(
        &app,
        request(Method::GET, "/arcaea/assets/app.js", None, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_value(&headers, header::CACHE_CONTROL),
        "public, max-age=31536000, immutable",
        "命中真实文件需长缓存"
    );
    assert_eq!(String::from_utf8_lossy(&bytes), "console.log(1)");
    assert!(
        header_value(&headers, header::CONTENT_TYPE).starts_with("text/javascript"),
        "需按扩展名给出 Content-Type"
    );
}

#[tokio::test]
async fn plugin_spa_falls_back_to_index_for_frontend_routes() {
    let fx = fixture("spa-route", false, false);
    let app = router(fx.state.clone());
    let (status, headers, bytes) =
        call(&app, request(Method::GET, "/arcaea/panels/1", None, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_value(&headers, header::CACHE_CONTROL),
        "no-cache",
        "前端路由回退 index.html 需 no-cache"
    );
    assert_eq!(
        header_value(&headers, header::CONTENT_TYPE),
        "text/html; charset=utf-8"
    );
    assert_eq!(String::from_utf8_lossy(&bytes), "<!doctype html>index");
    let (status, _, bytes) = call(&app, request(Method::GET, "/arcaea/", None, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(String::from_utf8_lossy(&bytes), "<!doctype html>index");
}

#[tokio::test]
async fn plugin_spa_missing_static_asset_is_real_404() {
    let fx = fixture("spa-missing", false, false);
    let app = router(fx.state.clone());
    let (status, headers, bytes) = call(
        &app,
        request(Method::GET, "/arcaea/assets/missing.js", None, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, ".js 未命中需 404 而不是回退");
    assert!(
        !String::from_utf8_lossy(&bytes).contains("doctype"),
        "未命中的静态资源不得回退 index.html"
    );
    assert!(headers.get(header::CACHE_CONTROL).is_none());
    let (status, _, _) = call(
        &app,
        request(Method::GET, "/arcaea/../secret.txt", None, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "越权路径需 404");
}

#[tokio::test]
async fn admin_api_unknown_path_returns_chinese_json() {
    let fx = fixture("api-fallback", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    for uri in ["/admin/api/nope", "/admin/api/menu/nope"] {
        let (status, headers, bytes) =
            call(&app, request(Method::GET, uri, Some(&token), None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} 需 404");
        assert_eq!(
            header_value(&headers, header::CONTENT_TYPE),
            "application/json",
            "{uri} 兜底需 JSON 而不是空体"
        );
        assert_eq!(json_body(&bytes)["error"], json!("接口不存在"));
    }
}

#[tokio::test]
async fn plugin_detail_config_and_access_alignment() {
    let fx = fixture("plugin-detail", false, false);
    let app = router(fx.state.clone());
    let token = login(&app).await;
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/plugins/arcaea", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["id"], json!("arcaea"));
    assert_eq!(json_body(&bytes)["version"], json!("1.0.0"));
    let (status, _, bytes) = call(
        &app,
        request(Method::GET, "/admin/api/plugins/nope", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json_body(&bytes)["error"], json!("插件不存在"));
    let body = json!({ "theme": "dark" });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/plugins/arcaea",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["ok"], json!(true));
    let (status, _, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/plugins/nope",
            Some(&token),
            Some(json!({ "a": 1 })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "未注册插件需 400");
    assert!(json_body(&bytes)["error"]
        .as_str()
        .expect("中文错误")
        .contains("not registered"));
    let body = json!({ "default": { "mode": "whitelist" } });
    let (status, _, bytes) = call(
        &app,
        request(
            Method::PUT,
            "/admin/api/plugins/arcaea/access",
            Some(&token),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&bytes)["ok"], json!(true));
    assert_eq!(fx.plugins.saved.lock().expect("锁插件配置").len(), 2);
}

#[test]
fn page_size_clamps_to_documented_bounds() {
    use aurorix_admin::inbox::{page_size, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};
    assert_eq!(page_size(None), DEFAULT_PAGE_SIZE, "缺省应当用默认页长");
    assert_eq!(
        page_size(Some(0)),
        DEFAULT_PAGE_SIZE,
        "零应当回落到默认页长"
    );
    assert_eq!(
        page_size(Some(-7)),
        DEFAULT_PAGE_SIZE,
        "负数应当回落到默认页长"
    );
    assert_eq!(page_size(Some(1)), 1, "正数应当被采用");
    assert_eq!(
        page_size(Some(MAX_PAGE_SIZE as i64)),
        MAX_PAGE_SIZE,
        "恰好等于上限时应当被采用"
    );
    assert_eq!(
        page_size(Some(MAX_PAGE_SIZE as i64 + 1)),
        DEFAULT_PAGE_SIZE,
        "超过上限时应当回落到默认页长"
    );
}
