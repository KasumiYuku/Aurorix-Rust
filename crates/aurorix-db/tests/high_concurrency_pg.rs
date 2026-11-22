use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use aurorix_core::Scene;
use aurorix_db::batch::{BatchConfig, InboxBatch};
use aurorix_db::inbox::{Direction, Message};
use aurorix_db::plugin_db::{MigrationStep, PluginDbRegistry, RegistryConfig};
use aurorix_db::stats::{ConvDelta, DayDelta, StatsBatch};
use aurorix_db::Db;
use chrono::Utc;
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
    scoped: PgPool,
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
            "hc_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        );
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        let scoped = Self::pool_in(&dsn, &schema, 2).await;
        Some(Self {
            admin,
            scoped,
            schema,
        })
    }

    async fn pool_in(dsn: &str, schema: &str, max_connections: u32) -> PgPool {
        let options = PgConnectOptions::from_str(dsn)
            .expect("解析连接串")
            .options([("search_path", schema)]);
        PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await
            .expect("连接沙箱")
    }

    async fn connect(&self) -> Db {
        let dsn = base_dsn().expect("测试库连接串");
        let pool = Self::pool_in(&dsn, &self.schema, 8).await;
        Db::from_pool(pool).await.expect("建表")
    }

    async fn execute(&self, sql: &str) {
        sqlx::query(sql).execute(&self.scoped).await.expect("执行");
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

fn conv_batch(name: &str, msg: i64) -> StatsBatch {
    let mut batch = StatsBatch::default();
    batch.conversations.insert(
        (Scene::Group, "g1".to_string()),
        ConvDelta {
            name: name.to_string(),
            msg,
            friend: false,
        },
    );
    batch
}

fn day_batch(key: &str, recv: i64, sent: i64, button: i64) -> StatsBatch {
    let mut batch = StatsBatch::default();
    batch
        .days
        .insert(key.to_string(), DayDelta { recv, sent, button });
    batch
}

fn message(conv: &str, content: &str, at_seconds: i64) -> Message {
    Message {
        id: 0,
        conv_id: conv.to_string(),
        kind: Scene::Group,
        direction: Direction::Inbound,
        sender_id: "u1".to_string(),
        sender_name: "阿明".to_string(),
        content: content.to_string(),
        platform_id: format!("p-{at_seconds}"),
        created_at: chrono::DateTime::<Utc>::from_timestamp(at_seconds, 0).expect("时间戳"),
    }
}

#[tokio::test]
async fn batched_stats_writes_land_exactly_once_with_summed_values() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let batcher = db.stats_batcher(BatchConfig::new().with_max_batch(4096));
    for _ in 0..3 {
        batcher.record(conv_batch("群一", 1)).await.expect("记录");
        batcher
            .record(day_batch("2024-05-01", 1, 2, 3))
            .await
            .expect("记录");
    }
    assert_eq!(batcher.pending_rows(), 2, "内存里应当合并成两行");
    let report = batcher.flush().await.expect("刷新");
    assert_eq!(report.rows, 2);
    assert_eq!(batcher.pending_rows(), 0);

    let row: (i64, String, bool) = sqlx::query_as(
        "SELECT msg_count, name, friend FROM stats_conversations WHERE scene = $1 AND id = $2",
    )
    .bind(Scene::Group.as_str())
    .bind("g1")
    .fetch_one(db.pool())
    .await
    .expect("读会话统计");
    assert_eq!(row.0, 3, "三次记录必须求和");
    assert_eq!(row.1, "群一");
    assert!(!row.2);

    let day: (i64, i64, i64) =
        sqlx::query_as("SELECT recv, sent, button FROM stats_daily WHERE day = $1")
            .bind("2024-05-01")
            .fetch_one(db.pool())
            .await
            .expect("读日统计");
    assert_eq!(day, (3, 6, 9));

    let again = batcher.flush().await.expect("再次刷新");
    assert_eq!(again.rows, 0, "没有新数据时不该写");
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM stats_conversations")
        .fetch_one(db.pool())
        .await
        .expect("计数");
    assert_eq!(total, 1, "重复刷新不能产生重复行");
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn batched_inbox_writes_land_exactly_once_and_merge_conversation() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let batcher = db.inbox_batcher(BatchConfig::new().with_max_batch(4096));
    let mut batch = InboxBatch::default();
    batch.push_message(message("c1", "第一条", 1_700_000_000));
    batch.push_message(message("c1", "第二条", 1_700_000_100));
    batch.note_conversation("c2", Scene::Group, 1_700_000_200, "另一条");
    assert_eq!(batch.rows(), 4);
    batcher.record(batch).await.expect("记录");
    let report = batcher.flush().await.expect("刷新");
    assert_eq!(report.rows, 4);

    let messages: i64 = sqlx::query_scalar("SELECT count(*) FROM inbox_messages")
        .fetch_one(db.pool())
        .await
        .expect("计数");
    assert_eq!(messages, 2);
    let conversations: i64 = sqlx::query_scalar("SELECT count(*) FROM inbox_conversations")
        .fetch_one(db.pool())
        .await
        .expect("计数");
    assert_eq!(conversations, 2);
    let c1: (i64, String) =
        sqlx::query_as("SELECT last_at, last_preview FROM inbox_conversations WHERE id = 'c1'")
            .fetch_one(db.pool())
            .await
            .expect("读会话");
    assert_eq!(c1.0, 1_700_000_100, "last_at 取最新");
    assert_eq!(c1.1, "第二条", "预览取最新一条");
    assert_eq!(batcher.flush().await.expect("再次刷新").rows, 0);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn unnest_path_writes_two_hundred_rows_per_statement() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let stats = db.stats_batcher(BatchConfig::new().with_max_batch(4096));
    let inbox = db.inbox_batcher(BatchConfig::new().with_max_batch(4096));
    let mut stats_batch = StatsBatch::default();
    let mut inbox_batch = InboxBatch::default();
    for index in 0..200i64 {
        stats_batch.conversations.insert(
            (Scene::Group, format!("g{index:03}")),
            ConvDelta {
                name: format!("群{index}"),
                msg: index + 1,
                friend: false,
            },
        );
        stats_batch.days.insert(
            format!("2024-07-{index:03}"),
            DayDelta {
                recv: 1,
                sent: 1,
                button: 0,
            },
        );
        inbox_batch.push_message(message(
            "bulk",
            &format!("第{index}条"),
            1_700_000_000 + index,
        ));
    }
    stats.record(stats_batch).await.expect("记录统计");
    inbox.record(inbox_batch).await.expect("记录收件箱");
    let stats_report = stats.flush().await.expect("刷新统计");
    let inbox_report = inbox.flush().await.expect("刷新收件箱");
    assert_eq!(stats_report.rows, 400);
    assert_eq!(inbox_report.rows, 201);

    let conversations: i64 = sqlx::query_scalar("SELECT count(*) FROM stats_conversations")
        .fetch_one(db.pool())
        .await
        .expect("计数");
    assert_eq!(conversations, 200);
    let days: i64 = sqlx::query_scalar("SELECT count(*) FROM stats_daily")
        .fetch_one(db.pool())
        .await
        .expect("计数");
    assert_eq!(days, 200);
    let messages: i64 = sqlx::query_scalar("SELECT count(*) FROM inbox_messages")
        .fetch_one(db.pool())
        .await
        .expect("计数");
    assert_eq!(messages, 200);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn shutdown_flushes_pending_rows_to_postgres() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let write_behind = db.write_behind(BatchConfig::new().with_max_batch(4096));
    write_behind
        .start_background(Duration::from_secs(3600))
        .expect("启动定时器");
    write_behind
        .stats
        .record(conv_batch("群一", 7))
        .await
        .expect("记录");
    let mut batch = InboxBatch::default();
    batch.push_message(message("c9", "关闭前的消息", 1_700_000_500));
    write_behind.inbox.record(batch).await.expect("记录");
    write_behind.shutdown().await.expect("关闭");
    assert_eq!(write_behind.stats_counters().pending, 0);
    assert_eq!(write_behind.inbox_counters().pending, 0);

    let msg_count: i64 =
        sqlx::query_scalar("SELECT msg_count FROM stats_conversations WHERE id = 'g1'")
            .fetch_one(db.pool())
            .await
            .expect("读统计");
    assert_eq!(msg_count, 7);
    let messages: i64 = sqlx::query_scalar("SELECT count(*) FROM inbox_messages")
        .fetch_one(db.pool())
        .await
        .expect("计数");
    assert_eq!(messages, 1);
    sandbox.drop_schema().await;
}

fn rating_steps() -> Vec<MigrationStep> {
    vec![
        MigrationStep {
            version: 2,
            name: "add_score",
            sql: "ALTER TABLE rating ADD COLUMN IF NOT EXISTS score BIGINT NOT NULL DEFAULT 0",
        },
        MigrationStep {
            version: 1,
            name: "create_rating",
            sql: "CREATE TABLE IF NOT EXISTS rating (uid TEXT PRIMARY KEY)",
        },
        MigrationStep {
            version: 3,
            name: "index_score",
            sql: "CREATE INDEX IF NOT EXISTS rating_score_idx ON rating(score)",
        },
    ]
}

#[tokio::test]
async fn plugin_migrations_apply_twice_without_duplicates() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let registry = PluginDbRegistry::new(db.pool().clone(), RegistryConfig::new(4, 2));
    let first = registry
        .migrate("rating", &rating_steps())
        .await
        .expect("首次迁移");
    assert_eq!(first.applied, vec![1, 2, 3], "必须按版本顺序应用");
    let second = registry
        .migrate("rating", &rating_steps())
        .await
        .expect("二次迁移");
    assert!(second.applied.is_empty(), "重复迁移不该再执行");
    assert_eq!(second.skipped, vec![1, 2, 3]);

    let recorded: i64 =
        sqlx::query_scalar("SELECT count(*) FROM plugin_migrations WHERE plugin_id = 'rating'")
            .fetch_one(db.pool())
            .await
            .expect("计数");
    assert_eq!(recorded, 3, "迁移记录不能重复");
    let columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = 'rating'",
    )
    .fetch_one(db.pool())
    .await
    .expect("计数");
    assert_eq!(columns, 2, "迁移必须真的建出表和列");
    let stats = registry.bulkhead_stats();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].migrations, vec![1, 2, 3]);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn plugin_kv_handles_stay_inside_their_own_scope() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let registry = PluginDbRegistry::new(db.pool().clone(), RegistryConfig::default());
    let rating = registry.plugin("rating").expect("句柄");
    rating.kv().set("theme", &"dark").await.expect("写插件 kv");
    assert_eq!(
        rating.kv().get::<String>("theme").await.expect("读插件 kv"),
        Some("dark".to_string())
    );
    assert_eq!(
        db.kv_global().get::<String>("theme").await.expect("读全局"),
        None,
        "插件不该写进全局作用域"
    );
    assert_eq!(
        db.kv_plugin("arcaea")
            .get::<String>("theme")
            .await
            .expect("读别的插件"),
        None,
        "插件之间必须隔离"
    );
    assert_eq!(rating.kv().scope(), aurorix_db::SCOPE_PLUGIN);
    let rows = rating
        .fetch_scalar_i64("SELECT count(*) FROM kv_data")
        .await
        .expect("带许可证的查询");
    assert_eq!(rows, Some(1), "只有插件自己的那一行可见");
    assert_eq!(rating.permits_in_use(), 0, "查询结束必须归还许可");
    assert_eq!(rating.available_permits(), rating.max_concurrent());
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn doctor_reports_a_fresh_schema_as_compatible() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let report = db.verify_schema().await.expect("体检");
    for table in &report.tables {
        println!("{}", table.summary);
    }
    println!("{}", report.summary);
    assert!(report.compatible, "{}", report.summary);
    assert_eq!(report.tables.len(), 6);
    assert_eq!(report.compatible_count(), 6);
    assert!(report.legacy_tables.is_empty(), "干净库不该有遗留表");
    let kv = report.table("kv_data").expect("kv_data 报告");
    assert!(!kv.missing_unique_key, "kv_data 必须有唯一键");
    assert!(kv.missing_columns.is_empty());
    assert!(kv.type_mismatches.is_empty());
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn doctor_detects_an_altered_table_and_recovers_after_drop() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    sandbox
        .execute("ALTER TABLE stats_daily ADD COLUMN legacy_flag BIGINT NOT NULL DEFAULT 0")
        .await;
    let report = db.verify_schema().await.expect("体检");
    println!("{}", report.summary);
    for table in &report.tables {
        println!("{}", table.summary);
    }
    assert!(!report.compatible);
    let daily = report.table("stats_daily").expect("stats_daily 报告");
    assert!(!daily.compatible);
    assert_eq!(daily.unexpected_columns, vec!["legacy_flag".to_string()]);
    assert!(
        daily.summary.contains("多出列 legacy_flag"),
        "{}",
        daily.summary
    );
    assert!(report
        .incompatible_tables()
        .iter()
        .any(|table| table.table == "stats_daily"));

    sandbox
        .execute("ALTER TABLE stats_daily DROP COLUMN legacy_flag")
        .await;
    let recovered = db.verify_schema().await.expect("复检");
    assert!(recovered.compatible, "{}", recovered.summary);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn doctor_reports_legacy_tables_without_failing() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    sandbox
        .execute("CREATE TABLE stats_groups (group_id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', msg_count BIGINT NOT NULL DEFAULT 0)")
        .await;
    sandbox
        .execute("CREATE TABLE legacy_peers (peer_id TEXT PRIMARY KEY)")
        .await;
    let report = db.verify_schema().await.expect("体检");
    println!("{}", report.summary);
    assert!(report.compatible, "{}", report.summary);
    assert_eq!(
        report.legacy_tables,
        vec!["legacy_peers".to_string(), "stats_groups".to_string()]
    );
    sandbox.drop_schema().await;
}
