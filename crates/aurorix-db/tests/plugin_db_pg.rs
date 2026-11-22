use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use aurorix_db::{Db, DbError, MigrationRegistry, MigrationStep, PluginDbRegistry, RegistryConfig};
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
            "pdb_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        );
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        Some(Self { admin, schema })
    }

    async fn pool(&self, max_connections: u32) -> PgPool {
        let dsn = base_dsn().expect("测试库连接串");
        let options = PgConnectOptions::from_str(&dsn)
            .expect("解析连接串")
            .options([("search_path", self.schema.as_str())]);
        PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await
            .expect("连接沙箱")
    }

    async fn db(&self) -> Db {
        Db::from_pool(self.pool(8).await).await.expect("建表")
    }

    async fn drop_schema(&self) {
        let _ = sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
            .execute(&self.admin)
            .await;
    }
}

async fn sandbox_or_skip() -> Option<Sandbox> {
    let Some(sandbox) = Sandbox::new().await else {
        eprintln!("跳过需要 PostgreSQL 的用例: 未设置 AURORIX_TEST_DATABASE");
        return None;
    };
    Some(sandbox)
}

const STEPS: &[MigrationStep] = &[
    MigrationStep {
        version: 1,
        name: "create_rating_table",
        sql:
            "CREATE TABLE IF NOT EXISTS rating_score (uid TEXT PRIMARY KEY, score BIGINT NOT NULL)",
    },
    MigrationStep {
        version: 2,
        name: "add_index",
        sql: "CREATE INDEX IF NOT EXISTS rating_score_idx ON rating_score (score)",
    },
];

#[tokio::test]
async fn migrations_record_versions_and_are_idempotent() {
    let Some(sandbox) = sandbox_or_skip().await else {
        return;
    };
    let pool = sandbox.pool(4).await;
    let registry = PluginDbRegistry::with_migrations(
        pool.clone(),
        RegistryConfig::new(2, 0),
        std::sync::Arc::new(MigrationRegistry::postgres(pool.clone())),
    );

    let first = registry.migrate("rating", STEPS).await.expect("首次迁移");
    assert_eq!(first.applied, vec![1, 2]);
    assert!(first.skipped.is_empty());
    assert_eq!(first.applied_count(), 2);

    let second = registry.migrate("rating", STEPS).await.expect("再次迁移");
    assert!(second.applied.is_empty());
    assert_eq!(second.skipped, vec![1, 2]);
    assert_eq!(second.skipped_count(), 2);

    let recorded: Vec<(i64, String)> = sqlx::query_as(
        "SELECT version, name FROM plugin_migrations WHERE plugin_id = $1 ORDER BY version",
    )
    .bind("rating")
    .fetch_all(&pool)
    .await
    .expect("读迁移记录");
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].0, 1);
    assert_eq!(recorded[0].1, "create_rating_table");
    assert_eq!(recorded[1].0, 2);

    let handle = registry.plugin("rating").expect("插件句柄");
    assert_eq!(handle.applied_versions(), vec![1, 2]);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn migration_advisory_lock_is_released_after_migrating() {
    let Some(sandbox) = sandbox_or_skip().await else {
        return;
    };
    let pool = sandbox.pool(4).await;
    let registry = PluginDbRegistry::with_migrations(
        pool.clone(),
        RegistryConfig::new(2, 0),
        std::sync::Arc::new(MigrationRegistry::postgres(pool.clone())),
    );
    registry.migrate("rating", STEPS).await.expect("迁移");

    let free: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(aurorix_db::MIGRATION_LOCK_KEY)
        .fetch_one(&pool)
        .await
        .expect("试探 advisory 锁");
    assert!(free, "迁移结束后锁必须被释放");
    let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
        .bind(aurorix_db::MIGRATION_LOCK_KEY)
        .fetch_one(&pool)
        .await
        .expect("释放试探锁");
    assert!(released, "试探锁要能解掉");
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn migration_ddl_is_replayable_and_keeps_existing_rows() {
    let Some(sandbox) = sandbox_or_skip().await else {
        return;
    };
    let pool = sandbox.pool(4).await;
    let registry = PluginDbRegistry::with_migrations(
        pool.clone(),
        RegistryConfig::new(2, 0),
        std::sync::Arc::new(MigrationRegistry::postgres(pool.clone())),
    );
    registry.migrate("rating", STEPS).await.expect("迁移");

    sqlx::query("INSERT INTO rating_score (uid, score) VALUES ($1, $2)")
        .bind("u1")
        .bind(12_i64)
        .execute(&pool)
        .await
        .expect("写入业务行");

    let again = registry.migrate("rating", STEPS).await.expect("重放迁移");
    assert!(again.applied.is_empty());

    let stored: i64 = sqlx::query_scalar("SELECT score FROM rating_score WHERE uid = $1")
        .bind("u1")
        .fetch_one(&pool)
        .await
        .expect("数据仍在");
    assert_eq!(stored, 12);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn bulkhead_rejects_beyond_configured_limit() {
    let Some(sandbox) = sandbox_or_skip().await else {
        return;
    };
    let pool = sandbox.pool(4).await;
    let registry = PluginDbRegistry::with_migrations(
        pool.clone(),
        RegistryConfig::new(2, 0),
        std::sync::Arc::new(MigrationRegistry::postgres(pool.clone())),
    );
    let handle = registry.plugin("rating").expect("插件句柄");
    assert_eq!(handle.max_concurrent(), 2);
    assert_eq!(handle.available_permits(), 2);

    let first = handle.try_acquire().expect("第一个许可");
    let second = handle.try_acquire().expect("第二个许可");
    assert_eq!(handle.permits_in_use(), 2);
    assert_eq!(handle.available_permits(), 0);

    let third = handle.try_acquire();
    match third {
        Err(DbError::PluginBulkheadFull { plugin, limit }) => {
            assert_eq!(plugin, "rating");
            assert_eq!(limit, 2);
        }
        other => panic!("第三个许可应当被拒绝, 实际为 {other:?}"),
    }
    assert_eq!(handle.metrics().rejections(), 1);

    drop(first);
    assert_eq!(handle.permits_in_use(), 1);
    let recovered = handle.try_acquire().expect("释放后可以再取");
    drop(recovered);
    drop(second);
    assert_eq!(handle.permits_in_use(), 0);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn plugin_kv_stays_inside_its_own_scope() {
    let Some(sandbox) = sandbox_or_skip().await else {
        return;
    };
    let db = sandbox.db().await;
    let registry = db.plugin_registry(RegistryConfig::new(2, 2));

    let rating = registry.plugin("rating").expect("rating 句柄");
    let phigros = registry.plugin("phigros").expect("phigros 句柄");

    rating
        .kv()
        .set("best", &"grievous lady")
        .await
        .expect("写 rating");
    assert_eq!(
        rating.kv().get::<String>("best").await.expect("读 rating"),
        Some("grievous lady".to_string())
    );
    assert_eq!(
        phigros
            .kv()
            .get::<String>("best")
            .await
            .expect("读 phigros"),
        None
    );

    db.kv_global().set("best", &"global").await.expect("写全局");
    assert_eq!(
        rating.kv().get::<String>("best").await.expect("读 rating"),
        Some("grievous lady".to_string())
    );

    let scopes: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT namespace FROM kv_data WHERE scope = 'plugin' ORDER BY namespace",
    )
    .fetch_all(db.pool())
    .await
    .expect("读 scope");
    assert_eq!(scopes, vec!["rating".to_string()]);

    let global_rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM kv_data WHERE scope = 'global'")
            .fetch_one(db.pool())
            .await
            .expect("统计全局");
    assert_eq!(global_rows, 1);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn plugin_query_runs_on_its_own_connection() {
    let Some(sandbox) = sandbox_or_skip().await else {
        return;
    };
    let db = sandbox.db().await;
    let registry = db.plugin_registry(RegistryConfig::new(2, 2));
    let handle = registry.plugin("rating").expect("插件句柄");

    let answer = handle
        .fetch_scalar_i64("SELECT 41::bigint + 1")
        .await
        .expect("标量查询");
    assert_eq!(answer, Some(42));

    handle
        .query("CREATE TABLE IF NOT EXISTS plugin_probe (id BIGINT)")
        .await
        .expect("建表");
    let affected = handle
        .query("INSERT INTO plugin_probe (id) VALUES (1), (2)")
        .await
        .expect("写入");
    assert_eq!(affected, 2);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn registry_reserves_connections_for_the_core() {
    let Some(sandbox) = sandbox_or_skip().await else {
        return;
    };
    let pool = sandbox.pool(8).await;
    let registry = PluginDbRegistry::with_migrations(
        pool.clone(),
        RegistryConfig::new(3, 3),
        std::sync::Arc::new(MigrationRegistry::postgres(pool.clone())),
    );
    assert_eq!(registry.reserved_core_connections(), 3);
    assert_eq!(registry.plugin_budget(), 5);
    assert_eq!(registry.remaining_budget(), 5);

    let first = registry.plugin("a").expect("a");
    assert_eq!(first.max_concurrent(), 3);
    assert_eq!(registry.total_permits(), 3);
    assert_eq!(registry.remaining_budget(), 2);

    let second = registry.plugin("b").expect("b");
    assert_eq!(second.max_concurrent(), 2);
    assert_eq!(registry.total_permits(), 5);
    assert_eq!(registry.remaining_budget(), 0);

    let third = registry.plugin("c").expect("c");
    assert_eq!(third.max_concurrent(), 1);
    assert_eq!(registry.known_plugins(), vec!["a", "b", "c"]);
    sandbox.drop_schema().await;
}
