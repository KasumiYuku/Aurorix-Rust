use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use aurorix_admin::assets::AssetProviderInput;
use aurorix_admin::{
    AssetSource, AssetsInput, BanEntry, BanSource, InboxSource, MenuSource, Panel, PanelItem,
    PgAssets, PgBans, PgInbox, PgMenu, SourceError,
};
use aurorix_db::{AdminFuture, AdminKv, AssetStore, BanStore, DbError, DbResult, PanelStore};
use axum::http::StatusCode;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fail {
    Invalid,
    Schema,
}

#[derive(Clone, Default)]
struct FakeKv {
    values: Arc<Mutex<BTreeMap<String, Value>>>,
    fail: Option<Fail>,
}

impl FakeKv {
    fn shared() -> Self {
        Self::default()
    }

    fn failing(fail: Fail) -> Self {
        Self {
            values: Arc::new(Mutex::new(BTreeMap::new())),
            fail: Some(fail),
        }
    }

    fn raw(&self, key: &str) -> Option<Value> {
        self.values.lock().expect("锁假 KV").get(key).cloned()
    }

    fn error(&self) -> Option<DbError> {
        match self.fail {
            Some(Fail::Invalid) => Some(DbError::Invalid("坏了".to_string())),
            Some(Fail::Schema) => Some(DbError::Schema("连接断了".to_string())),
            None => None,
        }
    }
}

impl AdminKv for FakeKv {
    fn get<'a>(&'a self, key: &'a str) -> AdminFuture<'a, DbResult<Option<Value>>> {
        Box::pin(async move {
            if let Some(err) = self.error() {
                return Err(err);
            }
            let value = { self.values.lock().expect("锁假 KV").get(key).cloned() };
            Ok(value)
        })
    }

    fn set<'a>(&'a self, key: &'a str, value: Value) -> AdminFuture<'a, DbResult<()>> {
        Box::pin(async move {
            if let Some(err) = self.error() {
                return Err(err);
            }
            self.values
                .lock()
                .expect("锁假 KV")
                .insert(key.to_string(), value);
            Ok(())
        })
    }

    fn remove<'a>(&'a self, key: &'a str) -> AdminFuture<'a, DbResult<()>> {
        Box::pin(async move {
            if let Some(err) = self.error() {
                return Err(err);
            }
            self.values.lock().expect("锁假 KV").remove(key);
            Ok(())
        })
    }
}

fn config(pairs: &[(&str, &str)]) -> Map<String, Value> {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert((*key).to_string(), Value::String((*value).to_string()));
    }
    map
}

fn command_item() -> PanelItem {
    PanelItem {
        name: "查询".to_string(),
        desc: "查分".to_string(),
        kind: "command".to_string(),
        only_admin: false,
        link: String::new(),
    }
}

#[test]
fn source_error_status_table_matches_api_errors() {
    let cases = [
        (
            SourceError::BadRequest("a".to_string()),
            StatusCode::BAD_REQUEST,
        ),
        (
            SourceError::NotFound("b".to_string()),
            StatusCode::NOT_FOUND,
        ),
        (SourceError::Conflict("c".to_string()), StatusCode::CONFLICT),
        (
            SourceError::Unavailable("d".to_string()),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            SourceError::Server("e".to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ];
    for (err, status) in cases {
        assert_eq!(err.status(), status);
        let api = aurorix_admin::ApiError::from(err.clone());
        assert_eq!(api.status(), status);
        assert_eq!(api.message(), err.message());
    }
}

#[test]
fn db_errors_map_to_bad_request_or_server() {
    let invalid = SourceError::from_db(DbError::Invalid("该用户不在封禁名单里".to_string()));
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(invalid.message(), "该用户不在封禁名单里");
    let broken = SourceError::from_db(DbError::Schema("连接断了".to_string()));
    assert_eq!(broken.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(broken.message().contains("存储失败"));
}

#[tokio::test]
async fn bans_adapter_writes_through_to_roster() {
    let kv = FakeKv::shared();
    let bans = PgBans::new(BanStore::new(Arc::new(kv.clone())));
    assert!(bans.list_async().await.expect("读空").is_empty());
    let entry = BanEntry {
        openid: "user-1".to_string(),
        message: "别再来了".to_string(),
        note: "刷屏".to_string(),
        at: String::new(),
    };
    bans.put_async(entry).await.expect("写封禁");
    let entries = bans.list_async().await.expect("读封禁");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].note, "刷屏");
    assert!(entries[0].at.contains('T'), "空 at 需补时间");
    let raw = kv.raw("roster").expect("roster 键");
    assert_eq!(raw[0]["openid"], json!("user-1"));
    assert_eq!(raw[0]["message"], json!("别再来了"));
    bans.delete_async("user-1".to_string()).await.expect("解封");
    assert!(bans.list_async().await.expect("读封禁").is_empty());
}

#[tokio::test]
async fn ban_adapter_errors_keep_status_codes() {
    let kv = FakeKv::shared();
    let bans = PgBans::new(BanStore::new(Arc::new(kv)));
    let err = bans
        .delete_async("user-404".to_string())
        .await
        .expect_err("不在名单");
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    assert_eq!(err.message(), "该用户不在封禁名单里");
    let oversized = "字".repeat(aurorix_admin::BAN_MESSAGE_LIMIT + 1);
    let err = bans
        .put_async(BanEntry {
            openid: "user-1".to_string(),
            message: oversized,
            note: String::new(),
            at: String::new(),
        })
        .await
        .expect_err("文案超限");
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    assert_eq!(err.message(), "文案 2001 字, 超过上限 2000 字");

    let broken = PgBans::new(BanStore::new(Arc::new(FakeKv::failing(Fail::Schema))));
    let err = broken.list_async().await.expect_err("存储不可用");
    assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(err.message().contains("存储失败"));
}

#[tokio::test]
async fn menu_adapter_persists_panels_and_snapshots() {
    let kv = FakeKv::shared();
    let menu = PgMenu::new(PanelStore::new(Arc::new(kv.clone())));
    let panel = Panel {
        items: vec![command_item()],
        remark: "说明".to_string(),
        version: 0,
    };
    let panel_id = menu
        .create_async("group".to_string(), panel.clone())
        .await
        .expect("新建面板");
    assert_eq!(menu.count_async().await.expect("计数"), 1);
    let current = menu
        .get_async(panel_id.clone())
        .await
        .expect("查面板")
        .expect("面板存在");
    assert_eq!(current.scope, "group");
    assert_eq!(current.target_type, "all");
    assert_eq!(current.version, 1);
    assert_eq!(current.panel.items[0].kind, "command");
    assert_eq!(
        menu.panels_async("group".to_string())
            .await
            .expect("按场景")
            .len(),
        1
    );
    assert!(menu
        .panels_async("c2c".to_string())
        .await
        .expect("别的场景")
        .is_empty());

    let file = menu
        .save_snapshot_async("update".to_string(), current.clone())
        .await
        .expect("留快照");
    assert!(file.ends_with(".json"), "快照名要能当路径段用: {file}");
    assert!(!file.contains('/'));
    let version = menu
        .update_async(
            panel_id.clone(),
            Panel {
                remark: "改过".to_string(),
                ..panel
            },
        )
        .await
        .expect("覆盖面板");
    assert_eq!(version, 2);

    let snapshots = menu.snapshots_async().await.expect("快照表");
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].action, "update");
    assert_eq!(snapshots[0].panel_id, panel_id);
    assert_eq!(snapshots[0].scope, "group");
    assert_eq!(snapshots[0].panel.remark, "说明");

    let read = menu
        .read_snapshot_async(file.clone())
        .await
        .expect("读快照")
        .expect("快照存在");
    assert_eq!(read.panel.remark, "说明");
    let restored = menu
        .restore_async(file)
        .await
        .expect("回滚")
        .expect("有快照");
    assert_eq!(restored.mode, "overwrite");
    assert_eq!(restored.scope, "group");
    assert_eq!(
        menu.get_async(panel_id.clone())
            .await
            .expect("查")
            .expect("存在")
            .panel
            .remark,
        "说明"
    );

    let raw = kv.raw("menu:panel").expect("menu:panel 键");
    assert_eq!(raw[&panel_id]["target_type"], json!("all"));
    assert!(kv.raw("menu:snapshot:index").is_some(), "需要快照索引");

    menu.delete_async(panel_id.clone()).await.expect("删面板");
    assert_eq!(menu.count_async().await.expect("计数"), 0);
    let err = menu.delete_async(panel_id).await.expect_err("再删");
    assert_eq!(err.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn assets_adapter_merges_registry_with_stored_config() {
    let kv = FakeKv::shared();
    let assets = PgAssets::new(AssetStore::new(Arc::new(kv.clone())));
    let empty = assets.view_async().await.expect("读空");
    assert_eq!(empty.providers.len(), 13, "内置图床必须全部出现在管理台");
    let qqbot = empty
        .providers
        .iter()
        .find(|item| item.name == "qqbot")
        .expect("qqbot 必须在列表里");
    assert!(qqbot.enabled, "qqbot 默认启用");
    assert!(!qqbot.configured);
    assert!(qqbot.schema.is_empty());
    let sgame = empty
        .providers
        .iter()
        .find(|item| item.name == "sgame")
        .expect("sgame 必须在列表里");
    assert!(!sgame.enabled);
    assert_eq!(sgame.schema.len(), 4);
    assert_eq!(sgame.schema[0].kind, "password");

    let input = AssetsInput {
        whitelist: vec!["cdn.example.com".to_string()],
        providers: vec![
            AssetProviderInput {
                name: "sgame".to_string(),
                enabled: false,
                priority: 20,
                config: config(&[
                    ("secretId", "AK"),
                    ("secretKey", "SK"),
                    ("host", "cos.example.com"),
                ]),
            },
            AssetProviderInput {
                name: "kurobbs".to_string(),
                enabled: true,
                priority: 10,
                config: Map::new(),
            },
        ],
    };
    assets.save_async(input).await.expect("写图床配置");
    let raw = kv.raw("assets:providers").expect("assets:providers 键");
    assert_eq!(raw[0]["name"], json!("kurobbs"), "按 priority 排序后落库");
    assert_eq!(raw[1]["name"], json!("sgame"));
    assert_eq!(raw[1]["config"]["secretKey"], json!("SK"));
    assert_eq!(kv.raw("assets:whitelist"), Some(json!(["cdn.example.com"])));

    let view = assets.view_async().await.expect("读资产配置");
    assert_eq!(view.whitelist, vec!["cdn.example.com".to_string()]);
    assert_eq!(view.providers.len(), 13);
    let sgame = view
        .providers
        .iter()
        .find(|item| item.name == "sgame")
        .expect("sgame 必须在列表里");
    assert!(sgame.configured);
    assert!(!sgame.enabled);
    assert_eq!(sgame.priority, 20);
    assert!(sgame.has_secrets);
    assert_eq!(sgame.config["secretKey"], json!(""), "密码字段必须打码");
    assert_eq!(sgame.config["host"], json!("cos.example.com"));

    let masked = AssetsInput {
        whitelist: view.whitelist.clone(),
        providers: view
            .providers
            .iter()
            .map(|item| AssetProviderInput {
                name: item.name.clone(),
                enabled: item.enabled,
                priority: item.priority,
                config: item.config.clone(),
            })
            .collect(),
    };
    assets.save_async(masked).await.expect("回写打码配置");
    let raw = kv.raw("assets:providers").expect("assets:providers 键");
    let stored = raw
        .as_array()
        .expect("落库必须是数组")
        .iter()
        .find(|item| item["name"] == json!("sgame"))
        .expect("sgame 必须落库")
        .clone();
    assert_eq!(
        stored["config"]["secretKey"],
        json!("SK"),
        "管理台留空的密码要用旧值补回"
    );

    let broken = PgAssets::new(AssetStore::new(Arc::new(FakeKv::failing(Fail::Invalid))));
    let err = broken.view_async().await.expect_err("存储不可用");
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn asset_provider_ids_collapse_repeated_separators() {
    let kv = FakeKv::default();
    let assets = PgAssets::new(AssetStore::new(Arc::new(kv.clone())));
    let input = AssetsInput {
        whitelist: Vec::new(),
        providers: vec![
            AssetProviderInput {
                name: "Foo -- Bar".to_string(),
                enabled: true,
                priority: 0,
                config: Map::new(),
            },
            AssetProviderInput {
                name: "Sprite-Sheet!!".to_string(),
                enabled: true,
                priority: 0,
                config: Map::new(),
            },
        ],
    };
    assets.save_async(input).await.expect("写图床配置");
    let raw = kv.raw("assets:providers").expect("assets:providers 键");
    assert_eq!(
        raw[0]["id"],
        json!("foo-bar"),
        "连续分隔符必须压成一个连字符"
    );
    assert_eq!(
        raw[1]["id"],
        json!("sprite-sheet"),
        "连续分隔符必须压成一个连字符"
    );
}

#[tokio::test]
async fn postgres_adapters_round_trip_when_database_is_set() {
    let Ok(dsn) = std::env::var("AURORIX_TEST_DATABASE") else {
        eprintln!("跳过需要 PostgreSQL 的用例: 未设置 AURORIX_TEST_DATABASE");
        return;
    };
    if dsn.is_empty() {
        return;
    }
    let db = Arc::new(aurorix_db::Db::connect(&dsn).await.expect("连接测试库"));
    let state = aurorix_admin::AdminState::new("config.json", aurorix_db::AppConfig::default())
        .with_database(db.clone());
    let via_state = state.bans.list_async().await.expect("读封禁");
    assert!(via_state.iter().all(|item| !item.openid.is_empty()));
    let _: &PgInbox = &PgInbox::new(db.inbox());
    let (bans, menu, assets, inbox) = aurorix_admin::postgres_sources(&db);

    bans.put_async(BanEntry {
        openid: "adapter-user-1".to_string(),
        message: "别再来了".to_string(),
        note: "集成测试".to_string(),
        at: String::new(),
    })
    .await
    .expect("写封禁");
    let entries = bans.list_async().await.expect("读封禁");
    assert!(entries.iter().any(|item| item.openid == "adapter-user-1"));
    bans.delete_async("adapter-user-1".to_string())
        .await
        .expect("解封");

    let panel_id = menu
        .create_async(
            "group".to_string(),
            Panel {
                items: vec![command_item()],
                remark: "集成".to_string(),
                version: 0,
            },
        )
        .await
        .expect("新建面板");
    let file = menu
        .save_snapshot_async(
            "update".to_string(),
            menu.get_async(panel_id.clone())
                .await
                .expect("查")
                .expect("面板存在"),
        )
        .await
        .expect("留快照");
    let version = menu
        .update_async(
            panel_id.clone(),
            Panel {
                items: vec![command_item()],
                remark: "改过".to_string(),
                version: 0,
            },
        )
        .await
        .expect("覆盖面板");
    assert_eq!(version, 2);
    let restored = menu
        .restore_async(file)
        .await
        .expect("回滚")
        .expect("有快照");
    assert_eq!(restored.mode, "overwrite");
    assert_eq!(
        menu.get_async(panel_id.clone())
            .await
            .expect("查")
            .expect("面板存在")
            .panel
            .remark,
        "集成"
    );
    menu.delete_async(panel_id.clone()).await.expect("删面板");

    assets
        .save_async(AssetsInput {
            whitelist: vec!["cdn.example.com".to_string()],
            providers: vec![AssetProviderInput {
                name: "本地".to_string(),
                enabled: true,
                priority: 0,
                config: config(&[("id", "local"), ("kind", "local")]),
            }],
        })
        .await
        .expect("写资产配置");
    let view = assets.view_async().await.expect("读资产配置");
    assert_eq!(view.whitelist, vec!["cdn.example.com".to_string()]);
    assert_eq!(view.providers[0].name, "本地");

    let conversations = inbox
        .conversations_async(aurorix_admin::DEFAULT_PAGE_SIZE)
        .await
        .expect("读会话");
    assert!(conversations.len() <= aurorix_admin::DEFAULT_PAGE_SIZE);
    assert!(inbox
        .messages_async("no-such-conversation".to_string(), None, 10)
        .await
        .expect("读消息")
        .is_empty());
    assert!(inbox
        .lookup_async("no-such-conversation".to_string())
        .await
        .expect("查会话")
        .is_none());
    let conversations_via_store = PgInbox::new(db.inbox())
        .conversations_async(5)
        .await
        .expect("读会话");
    assert!(conversations_via_store.len() <= 5);
}
