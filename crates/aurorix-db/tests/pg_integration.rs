use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use aurorix_core::Scene;
use aurorix_db::inbox::{Direction, Message};
use aurorix_db::kv::KvStore;
use aurorix_db::stats::StatsBatch;
use aurorix_db::{Db, DbError};
use chrono::{Duration, Utc};
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
            "t_{}_{}",
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

#[tokio::test]
async fn creates_exactly_the_six_framework_tables() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    for table in [
        "kv_data",
        "stats_conversations",
        "stats_daily",
        "stats_migrations",
        "inbox_conversations",
        "inbox_messages",
    ] {
        let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(table)
            .fetch_one(db.pool())
            .await
            .expect("查询表");
        assert!(exists, "{table} 未创建");
    }
    let columns: Vec<(String, String)> = sqlx::query_as(
        "SELECT column_name, data_type FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = 'kv_data' ORDER BY ordinal_position",
    )
    .fetch_all(db.pool())
    .await
    .expect("查列");
    let names: Vec<&str> = columns.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec!["scope", "namespace", "key", "value", "updated_at"]
    );
    let updated_at = &columns[4].1;
    assert_eq!(updated_at, "timestamp with time zone");
    let value_type = &columns[3].1;
    assert_eq!(value_type, "bytea");

    let identity: Option<String> = sqlx::query_scalar(
        "SELECT column_default FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = 'inbox_messages' AND column_name = 'id'",
    )
    .fetch_one(db.pool())
    .await
    .expect("查自增");
    assert!(identity.is_none(), "identity 列不应有 default 表达式");

    sandbox.drop_schema().await;
}

#[tokio::test]
async fn kv_round_trip_and_namespace_isolation() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let store = KvStore::plugin(db.pool().clone(), "arcaea");
    assert_eq!(store.scope(), "plugin");
    assert_eq!(store.namespace(), "arcaea");
    store
        .set("profile", &serde_json::json!({"uid": "u1", "name": "阿明"}))
        .await
        .expect("写");
    let value: Option<serde_json::Value> = store.get("profile").await.expect("读");
    assert_eq!(value.expect("有值")["name"], serde_json::json!("阿明"));
    assert!(store.has("profile").await.expect("has"));
    assert!(!store.has("absent").await.expect("has"));

    let other = KvStore::plugin(db.pool().clone(), "phigros");
    assert!(other
        .get::<serde_json::Value>("profile")
        .await
        .expect("读")
        .is_none());

    store.delete("profile").await.expect("删");
    assert!(!store.has("profile").await.expect("has"));

    store.set("a", &1i64).await.expect("写");
    store.set("b", &2i64).await.expect("写");
    store.clear().await.expect("清空");
    assert!(!store.has("a").await.expect("has"));
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn kv_incr_is_atomic_across_concurrent_tasks() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let store = KvStore::scene(db.pool().clone(), Scene::Group, "g1");
    let mut handles = Vec::new();
    for _ in 0..8 {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..5 {
                store.incr("footer.+1", 1).await.expect("自增");
            }
        }));
    }
    for handle in handles {
        handle.await.expect("任务");
    }
    let total: Option<i64> = store.get("footer.+1").await.expect("读");
    assert_eq!(total, Some(40));
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn kv_incr_rejects_non_integer_payloads() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let store = KvStore::global(db.pool().clone());
    store
        .set("counter", &serde_json::json!({"not": "a number"}))
        .await
        .expect("写");
    let err = store.incr("counter", 1).await.unwrap_err();
    assert!(matches!(err, DbError::NotAnInteger(_, _)), "{err}");
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn inbox_saves_messages_and_aggregates_conversation() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let inbox = db.inbox();
    let now = Utc::now();
    let message = Message {
        id: 0,
        conv_id: "g1".to_string(),
        kind: Scene::Group,
        direction: Direction::Inbound,
        sender_id: "u1".to_string(),
        sender_name: "阿明".to_string(),
        content: "  你好\n世界  ".to_string(),
        platform_id: "m1".to_string(),
        created_at: now,
    };
    inbox.save(&message).await.expect("存消息");
    let conversations = inbox.conversations().await.expect("会话列表");
    assert_eq!(conversations.len(), 1);
    assert_eq!(conversations[0].id, "g1");
    assert_eq!(conversations[0].kind, Scene::Group);
    assert_eq!(conversations[0].last_preview, "你好 世界");
    assert!(conversations[0].last_at.is_some());

    let found = inbox.lookup("g1").await.expect("查会话").expect("存在");
    assert_eq!(found.last_preview, "你好 世界");
    assert!(inbox.lookup("absent").await.expect("查会话").is_none());

    let history = inbox.messages("g1", 0, 0).await.expect("历史");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].direction, Direction::Inbound);
    assert_eq!(history[0].content, "  你好\n世界  ");

    inbox.set_name("g1", "测试群").await.expect("写名");
    assert_eq!(inbox.name_of("g1").await.expect("读名"), "测试群");
    inbox
        .set_name("g1", "改名后的群")
        .await
        .expect("群改名要能覆盖旧名");
    assert_eq!(
        inbox.name_of("g1").await.expect("读名"),
        "改名后的群",
        "群改名之后会话名要跟着更新"
    );

    let old = Message {
        id: 0,
        conv_id: "g1".to_string(),
        kind: Scene::Group,
        direction: Direction::Outbound,
        sender_id: String::new(),
        sender_name: String::new(),
        content: "很久以前".to_string(),
        platform_id: String::new(),
        created_at: now - Duration::days(120),
    };
    inbox.save(&old).await.expect("存旧消息");
    inbox.trim().await.expect("清理");
    let history = inbox.messages("g1", 0, 50).await.expect("历史");
    assert_eq!(history.len(), 1);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn stats_batch_accumulates_and_counts_conversations() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let stats = db.stats();
    let mut batch = StatsBatch::default();
    batch.received(Scene::Group, "g1", "测试群");
    batch.received(Scene::Group, "g1", "改名");
    batch.received(Scene::C2c, "u1", "");
    batch.count_day("2024-01-01", 5, 3, 2);
    batch.trim = false;
    stats.write_batch(&batch).await.expect("落库");

    let counts = stats.read_counts().await.expect("统计");
    assert_eq!(counts.groups, 1);
    assert_eq!(counts.peers, 1);
    assert_eq!(counts.friends, 1);
    assert_eq!(counts.channels, 0);
    assert_eq!(counts.top.len(), 2);
    assert_eq!(counts.top[0].id, "g1");
    assert_eq!(counts.top[0].msg, 2);
    assert_eq!(counts.top[0].name, "测试群");

    stats.friend("u1").await.expect("好友");
    let counts = stats.read_counts().await.expect("统计");
    assert_eq!(counts.friends, 1);
    stats.unfriend("u1").await.expect("删好友");
    let counts = stats.read_counts().await.expect("统计");
    assert_eq!(counts.friends, 0);
    assert_eq!(counts.peers, 1);

    let (totals, day, today) = stats.seed_totals().await.expect("汇总");
    assert_eq!(totals.recv, 5);
    assert_eq!(day.len(), 10);
    assert_eq!(today.recv, 0);

    let view = stats
        .snapshot(
            aurorix_db::LifeTotals::default(),
            aurorix_db::DayStat::default(),
        )
        .await
        .expect("快照");
    assert_eq!(view.groups, 1);
    assert_eq!(view.top_conversations.len(), 2);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn schema_creation_is_idempotent() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    db.ensure_schema().await.expect("二次建表");
    db.ensure_schema().await.expect("三次建表");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = current_schema() AND table_name = 'kv_data'",
    )
    .fetch_one(db.pool())
    .await
    .expect("统计表");
    assert_eq!(count, 1);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn stats_set_name_writes_group_name_without_touching_counts() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let stats = db.stats();
    stats
        .set_name(Scene::Group, "g-1", "改名前")
        .await
        .expect("写群名");
    stats
        .set_name(Scene::Group, "g-1", "改名后")
        .await
        .expect("覆盖群名");
    let view = stats.full_view().await.expect("读统计");
    let row = view
        .top_conversations
        .iter()
        .find(|row| row.id == "g-1")
        .expect("会话要出现在活跃列表里");
    assert_eq!(row.name, "改名后", "群改名后统计里的会话名要更新");
    assert_eq!(row.msg, 0, "写名字不该增加消息数");
    sandbox.drop_schema().await;
}
