use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use aurorix_db::{
    verify_existing_schema, AssetStore, BanStore, KvStore, ASSET_PROVIDERS_KEY, ROSTER_KEY,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

const GO_SCHEMA: &str = include_str!("fixtures/go_legacy_schema.sql");

const GO_ASSETS: &str = include_str!("fixtures/go_assets_providers.json");

const GO_ROSTER: &str = r#"[{"openid":"6A1F2C3D4E5F67890123456789ABCDEF","message":"别再刷屏了","note":"刷屏","at":"2026-02-14T15:04:05+08:00"},{"openid":"0B2C3D4E5F60718293A4B5C6D7E8F901","at":"2026-02-13T09:00:00+08:00"},{"openid":"11223344556677889900112233445566","message":""}]"#;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn base_dsn() -> Option<String> {
    std::env::var("AURORIX_TEST_DATABASE")
        .ok()
        .filter(|dsn| !dsn.is_empty())
}

struct Sandbox {
    admin: PgPool,
    pool: PgPool,
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
            "go_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        );
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        let options = PgConnectOptions::from_str(&dsn)
            .expect("解析连接串")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("连接沙箱");
        let sandbox = Self {
            admin,
            pool,
            schema,
        };
        sandbox.apply_go_schema().await;
        Some(sandbox)
    }

    async fn apply_go_schema(&self) {
        sqlx::raw_sql(GO_SCHEMA)
            .execute(&self.pool)
            .await
            .expect("执行 Go 版建表脚本");
    }

    async fn exec(&self, sql: &str) {
        sqlx::query(sql)
            .execute(&self.pool)
            .await
            .expect("执行 SQL");
    }

    async fn put_kv(&self, key: &str, value: &str) {
        sqlx::query(
            "INSERT INTO kv_data (scope, namespace, key, value) VALUES ('global', 'global', $1, $2)",
        )
        .bind(key)
        .bind(value.as_bytes().to_vec())
        .execute(&self.pool)
        .await
        .expect("写入历史键值");
    }

    async fn drop_schema(&self) {
        let _ = sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
            .execute(&self.admin)
            .await;
    }
}

async fn sandbox() -> Option<Sandbox> {
    let sandbox = Sandbox::new().await;
    if sandbox.is_none() {
        eprintln!("跳过需要 PostgreSQL 的用例: 未设置 AURORIX_TEST_DATABASE");
    }
    sandbox
}

#[tokio::test]
async fn go_created_database_passes_the_read_only_doctor() {
    let Some(box_) = sandbox().await else {
        return;
    };
    let report = verify_existing_schema(&box_.pool)
        .await
        .expect("体检应当能读库");
    assert!(report.compatible, "{}", report.summary);
    assert_eq!(report.tables.len(), 6);
    assert_eq!(report.compatible_count(), 6);
    assert!(
        report.legacy_tables.is_empty(),
        "{:?}",
        report.legacy_tables
    );
    for name in [
        "kv_data",
        "stats_conversations",
        "stats_daily",
        "stats_migrations",
        "inbox_conversations",
        "inbox_messages",
    ] {
        let table = report.table(name).expect("核心表要在报告里");
        assert!(table.present, "{name} 应当存在");
        assert!(table.missing_columns.is_empty(), "{name}: {:?}", table);
        assert!(table.type_mismatches.is_empty(), "{name}: {:?}", table);
        assert!(table.missing_indexes.is_empty(), "{name}: {:?}", table);
        assert_eq!(table.row_count, Some(0));
    }
    box_.drop_schema().await;
}

#[tokio::test]
async fn doctor_reports_missing_columns_and_indexes_by_name() {
    let Some(box_) = sandbox().await else {
        return;
    };
    box_.exec("ALTER TABLE stats_conversations DROP COLUMN friend")
        .await;
    box_.exec("DROP INDEX inbox_messages_at").await;
    let report = verify_existing_schema(&box_.pool)
        .await
        .expect("体检应当能读库");
    assert!(!report.compatible);
    let conversations = report.table("stats_conversations").expect("表报告");
    assert!(conversations
        .missing_columns
        .contains(&"friend".to_string()));
    assert!(!conversations.compatible);
    let messages = report.table("inbox_messages").expect("表报告");
    assert_eq!(
        messages.missing_indexes,
        vec!["inbox_messages_at".to_string()]
    );
    assert!(messages.compatible, "缺索引属于建议项, 不影响数据兼容");
    assert!(
        messages.summary.contains("建议补建索引 inbox_messages_at"),
        "{}",
        messages.summary
    );
    assert_eq!(report.incompatible_tables().len(), 1);
    box_.drop_schema().await;
}

#[tokio::test]
async fn legacy_kv_payloads_decode_without_any_loss() {
    let Some(box_) = sandbox().await else {
        return;
    };
    box_.put_kv(ROSTER_KEY, GO_ROSTER).await;
    box_.put_kv(ASSET_PROVIDERS_KEY, GO_ASSETS).await;
    let kv = KvStore::global(box_.pool.clone());

    let roster = BanStore::from_kv(kv.clone())
        .load()
        .await
        .expect("老库封禁名单要能解析");
    assert_eq!(roster.len(), 3);
    let first = roster
        .iter()
        .find(|entry| entry.openid == "6A1F2C3D4E5F67890123456789ABCDEF")
        .expect("带文案的那条");
    assert_eq!(first.message, "别再刷屏了");
    assert_eq!(first.note, "刷屏");
    assert_eq!(first.at, "2026-02-14T15:04:05+08:00");
    assert_eq!(first.text(), "别再刷屏了");
    let bare = roster
        .iter()
        .find(|entry| entry.openid == "0B2C3D4E5F60718293A4B5C6D7E8F901")
        .expect("省略文案与备注的那条");
    assert!(bare.message.is_empty());
    assert!(bare.note.is_empty());
    assert_eq!(bare.text(), aurorix_db::DEFAULT_BAN_MESSAGE);
    let empty_message = roster
        .iter()
        .find(|entry| entry.openid == "11223344556677889900112233445566")
        .expect("空文案的那条");
    assert_eq!(empty_message.text(), aurorix_db::DEFAULT_BAN_MESSAGE);
    assert_eq!(roster[0].at, "2026-02-14T15:04:05+08:00");

    let providers = AssetStore::from_kv(kv.clone())
        .load()
        .await
        .expect("老库图床配置要能解析");
    assert_eq!(providers.len(), 11);
    let sgame = &providers[0];
    assert_eq!(sgame.name, "sgame");
    assert!(sgame.enabled);
    assert_eq!(sgame.priority, 300);
    assert!(sgame.probe.is_none(), "老库没有 probe 字段");
    assert_eq!(
        sgame
            .config
            .get("bucketUrl")
            .and_then(|value| value.as_str()),
        Some("https://download.nature.qq.com")
    );
    let chatglm = providers
        .iter()
        .find(|provider| provider.name == "chatglm")
        .expect("缺 config 字段的那条");
    assert!(!chatglm.enabled);
    assert_eq!(chatglm.priority, 200);
    assert!(chatglm.config.is_empty(), "缺字段要落成空对象");
    let smms = providers
        .iter()
        .find(|provider| provider.name == "smms")
        .expect("未知 Provider 名称也要能读出来");
    assert_eq!(smms.priority, 0);
    let duplicated: Vec<&str> = providers
        .iter()
        .filter(|provider| provider.name == "sgame")
        .map(|provider| provider.name.as_str())
        .collect();
    assert_eq!(duplicated.len(), 2, "老库有重复的 sgame 条目");

    let raw = kv
        .get::<serde_json::Value>(ASSET_PROVIDERS_KEY)
        .await
        .expect("直接读值")
        .expect("值存在");
    assert_eq!(raw.as_array().map(Vec::len), Some(11));
    box_.drop_schema().await;
}

#[tokio::test]
async fn rewriting_a_legacy_payload_keeps_the_wire_shape() {
    let Some(box_) = sandbox().await else {
        return;
    };
    box_.put_kv(ASSET_PROVIDERS_KEY, GO_ASSETS).await;
    let kv = KvStore::global(box_.pool.clone());
    let store = AssetStore::from_kv(kv.clone());
    let providers = store.load().await.expect("读老值");
    store.save(&providers).await.expect("写回");
    let written: serde_json::Value = kv
        .get(ASSET_PROVIDERS_KEY)
        .await
        .expect("读回")
        .expect("值存在");
    let first = &written.as_array().expect("数组")[0];
    assert_eq!(first["name"], "sgame");
    assert_eq!(first["priority"], 300);
    assert!(
        first.get("probe").is_none(),
        "未设置 probe 时不应写出该字段"
    );
    let chatglm = written
        .as_array()
        .expect("数组")
        .iter()
        .find(|item| item["name"] == "chatglm")
        .expect("chatglm 条目");
    assert!(chatglm.get("config").is_none(), "空 config 不应写出");
    box_.drop_schema().await;
}
