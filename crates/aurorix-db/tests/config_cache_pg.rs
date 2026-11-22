use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use aurorix_db::{ConfigCache, Db, KvStore};
use serde_json::json;
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
            "cfg_{}_{}",
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
async fn load_seeds_cache_and_reads_stay_in_memory() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let store = db.kv_global();
    store.set("nick", &json!("阿凯")).await.expect("写 nick");
    store.set("limit", &json!(20)).await.expect("写 limit");
    store
        .set("flags", &json!({ "beta": true }))
        .await
        .expect("写 flags");

    let cache = db.config_cache();
    assert!(!cache.is_loaded());
    let loaded = cache.load().await.expect("装载配置");
    assert_eq!(loaded, 3);
    assert_eq!(cache.len(), 3);
    assert!(cache.is_loaded());

    let nick: Option<String> = cache.get("nick").await.expect("读 nick");
    assert_eq!(nick.as_deref(), Some("阿凯"));
    let limit: Option<i64> = cache.get("limit").await.expect("读 limit");
    assert_eq!(limit, Some(20));
    let hits_before = cache.stats().hits;

    store
        .set("nick", &json!("改名了"))
        .await
        .expect("绕过缓存在库里改");

    let cached: Option<String> = cache.get("nick").await.expect("读缓存");
    assert_eq!(cached.as_deref(), Some("阿凯"));
    assert_eq!(cache.stats().hits, hits_before + 1);

    cache.reload().await.expect("重新装载");
    let refreshed: Option<String> = cache.get("nick").await.expect("读新值");
    assert_eq!(refreshed.as_deref(), Some("改名了"));
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn set_is_write_through_and_delete_purges_both_sides() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let cache = db.config_cache();
    cache.load().await.expect("装载空配置");

    cache
        .set("greeting", &json!({ "text": "你好" }))
        .await
        .expect("写穿透");
    assert_eq!(cache.len(), 1);

    let raw = cache.get_raw("greeting").expect("缓存里应该有");
    assert_eq!(raw.get("text").and_then(|v| v.as_str()), Some("你好"));

    let from_db: Option<serde_json::Value> = db.kv_global().get("greeting").await.expect("查库");
    assert_eq!(
        from_db
            .as_ref()
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str()),
        Some("你好")
    );

    cache.set_raw("counter", json!(7)).await.expect("写裸值");
    assert_eq!(cache.len(), 2);

    cache.delete("greeting").await.expect("删除");
    assert_eq!(cache.len(), 1);
    assert!(cache.get_raw("greeting").is_none());

    let gone: Option<serde_json::Value> = db.kv_global().get("greeting").await.expect("查库");
    assert!(gone.is_none());
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn miss_falls_back_to_database_and_populates_cache() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let store: KvStore = db.kv_global();
    let cache: std::sync::Arc<ConfigCache> = db.config_cache();
    cache.load().await.expect("装载空配置");
    assert!(cache.is_empty());

    store
        .set("theme", &json!("dark"))
        .await
        .expect("绕过缓存在库里写");
    assert!(cache.get_raw("theme").is_none());

    let theme: Option<String> = cache.get("theme").await.expect("读穿透");
    assert_eq!(theme.as_deref(), Some("dark"));
    assert!(cache.get_raw("theme").is_some());

    let unknown: Option<String> = cache.get("nope").await.expect("读不存在");
    assert!(unknown.is_none());
    assert_eq!(cache.len(), 1);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn byte_budget_stops_oversized_namespace_from_loading() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let store = db.kv_global();
    for index in 0..8 {
        store
            .set(&format!("key-{index}"), &json!("x".repeat(512)))
            .await
            .expect("写配置");
    }

    let cache = ConfigCache::new(store).with_limit(1200);
    let loaded = cache.load().await.expect("装载");
    assert!(loaded < 8);
    assert!(loaded >= 1);
    assert_eq!(cache.stats().entries, loaded);
    sandbox.drop_schema().await;
}

#[tokio::test]
async fn seed_and_invalidate_all_work_without_database() {
    let Some((sandbox, db)) = sandbox_or_skip().await else {
        return;
    };
    let cache = db.config_cache();
    cache.seed(vec![
        ("a".to_string(), json!(1)),
        ("b".to_string(), json!(2)),
    ]);
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.keys(), vec!["a".to_string(), "b".to_string()]);

    cache.store_raw("c", json!(3));
    assert_eq!(cache.len(), 3);

    cache.invalidate("a");
    assert_eq!(cache.len(), 2);
    assert!(cache.get_raw("a").is_none());

    cache.invalidate_all();
    assert!(cache.is_empty());
    assert_eq!(cache.keys().len(), 0);
    sandbox.drop_schema().await;
}
