use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use aurorix_db::admin::{AssetProvider, BanEntry, PanelBody, PanelItem, PANEL_SNAPSHOT_LIMIT};
use aurorix_db::Db;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn base_dsn() -> Option<String> {
    std::env::var("AURORIX_TEST_DATABASE")
        .ok()
        .filter(|dsn| !dsn.is_empty())
}

struct Sandbox {
    admin: PgPool,
    schema: String,
}

impl Sandbox {
    async fn new() -> Option<Self> {
        let dsn = base_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&dsn)
            .await
            .ok()?;
        let schema = format!(
            "adm_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        );
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        Some(Self { admin, schema })
    }

    async fn connect(&self) -> Db {
        let dsn = base_dsn().expect("测试库连接串");
        let options = PgConnectOptions::from_str(&dsn)
            .expect("解析连接串")
            .options([("search_path", self.schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("连接沙箱");
        Db::from_pool(pool).await.expect("建表")
    }

    async fn drop_schema(&self) {
        let _ = sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
            .execute(&self.admin)
            .await;
    }
}

async fn sandbox_or_skip() -> Option<(Sandbox, Db)> {
    let Some(sandbox) = Sandbox::new().await else {
        eprintln!("跳过需要 PostgreSQL 的用例: 未设置 AURORIX_TEST_DATABASE");
        return None;
    };
    let db = sandbox.connect().await;
    Some((sandbox, db))
}

fn item(name: &str) -> PanelItem {
    PanelItem {
        name: name.to_string(),
        desc: "查分".to_string(),
        kind: "command".to_string(),
        ..PanelItem::default()
    }
}

#[tokio::test]
async fn ban_store_round_trip_in_postgres() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let store = db.bans();
    assert!(store.load().await.expect("读空名单").is_empty());
    store
        .add(
            BanEntry::new("user-1")
                .with_message("别再来了")
                .with_note("刷屏")
                .with_at("2024-01-01T00:00:00Z"),
        )
        .await
        .expect("写封禁");
    store.add(BanEntry::new("user-2")).await.expect("写封禁");
    let entries = store.load().await.expect("读封禁");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].openid, "user-2", "新封的排最前");
    assert!(entries[0].at.contains('T'), "空 at 需补时间");
    let raw: Vec<u8> =
        sqlx::query_scalar("SELECT value FROM kv_data WHERE scope = 'global' AND key = 'roster'")
            .fetch_one(db.pool())
            .await
            .expect("查 roster 键");
    let decoded: serde_json::Value = serde_json::from_slice(&raw).expect("解析 roster");
    assert_eq!(decoded[0]["openid"], serde_json::json!("user-1"));
    store.remove("user-1").await.expect("解封");
    assert_eq!(store.load().await.expect("读封禁").len(), 1);
    assert_eq!(
        store.remove("user-404").await.unwrap_err().to_string(),
        "该用户不在封禁名单里"
    );
    store.clear().await.expect("清空");
    assert!(store.load().await.expect("读封禁").is_empty());
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn panel_store_round_trip_in_postgres() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let store = db.panels();
    let created = store
        .create(
            "group",
            PanelBody {
                items: vec![item("查询")],
                remark: "面板".to_string(),
                version: 0,
            },
        )
        .await
        .expect("新建面板");
    assert_eq!(created.version, 1);
    assert_eq!(created.target_type, "all");
    let listed = store.list("group").await.expect("按场景列表");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].panel.items[0].kind, "command");
    assert!(store.list("c2c").await.expect("别的场景").is_empty());
    assert_eq!(store.count().await.expect("计数"), 1);

    let snapshot = store
        .push_snapshot(&created, "update")
        .await
        .expect("留快照");
    assert_eq!(snapshot.seq, 1);
    let mut edit = created.clone();
    edit.panel.remark = "改过".to_string();
    let updated = store.upsert(edit).await.expect("覆盖");
    assert_eq!(updated.version, 2);

    let snapshots = store.snapshots(&created.panel_id).await.expect("快照表");
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].action, "update");
    let found = store
        .snapshot(&snapshot.name)
        .await
        .expect("按名查快照")
        .expect("快照存在");
    assert_eq!(found.record.panel.remark, "面板");

    let restored = store.restore(&created.panel_id, 1).await.expect("回滚快照");
    assert_eq!(restored.panel.remark, "面板");
    assert_eq!(restored.version, 3);

    for _ in 0..(PANEL_SNAPSHOT_LIMIT + 3) {
        store
            .push_snapshot(&restored, "update")
            .await
            .expect("留快照");
    }
    assert_eq!(
        store
            .snapshots(&created.panel_id)
            .await
            .expect("快照表")
            .len(),
        PANEL_SNAPSHOT_LIMIT,
        "每个面板最多保留 20 份快照"
    );

    assert!(store.remove(&created.panel_id).await.expect("删面板"));
    assert!(!store.remove(&created.panel_id).await.expect("再删"));
    assert_eq!(store.count().await.expect("计数"), 0);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn asset_store_round_trip_in_postgres() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let store = db.assets();
    assert!(store.load().await.expect("读空数组").is_empty());
    let providers = vec![
        AssetProvider {
            id: "local".to_string(),
            name: "本地".to_string(),
            kind: "local".to_string(),
            base_url: "https://cdn.example.com".to_string(),
            enabled: true,
            ..AssetProvider::default()
        },
        AssetProvider {
            id: "remote".to_string(),
            name: "远端".to_string(),
            kind: "http".to_string(),
            base_url: String::new(),
            enabled: false,
            ..AssetProvider::default()
        },
    ];
    store.save(&providers).await.expect("写 provider");
    assert_eq!(store.load().await.expect("读 provider"), providers);
    store
        .save_whitelist(&["cdn.example.com".to_string()])
        .await
        .expect("写白名单");
    assert_eq!(
        store.load_whitelist().await.expect("读白名单"),
        vec!["cdn.example.com".to_string()]
    );
    store.save(&[]).await.expect("清空 provider");
    assert!(store.load().await.expect("读 provider").is_empty());
    sandbox.drop_schema().await;
}
