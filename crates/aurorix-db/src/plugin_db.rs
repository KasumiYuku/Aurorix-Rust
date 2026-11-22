use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use sqlx::PgPool;
use tokio::sync::OwnedSemaphorePermit;

use crate::error::{DbError, DbResult};
use crate::kv::KvStore;

pub const PLUGIN_MIGRATIONS_TABLE: &str = "plugin_migrations";

pub const PLUGIN_MIGRATIONS_DDL: &str = "CREATE TABLE IF NOT EXISTS plugin_migrations (\n\tplugin_id  TEXT NOT NULL,\n\tversion    BIGINT NOT NULL,\n\tname       TEXT NOT NULL,\n\tapplied_at TIMESTAMPTZ NOT NULL DEFAULT now(),\n\tPRIMARY KEY (plugin_id, version)\n)";

pub const PLUGIN_MIGRATION_VERSIONS: &str =
    "SELECT version FROM plugin_migrations WHERE plugin_id = $1 ORDER BY version";

pub const PLUGIN_MIGRATION_RECORD: &str = "INSERT INTO plugin_migrations (plugin_id, version, name) VALUES ($1, $2, $3) ON CONFLICT (plugin_id, version) DO NOTHING";

pub const MIGRATION_LOCK_KEY: i64 = 0x6175_726F_7269_7801;

pub const MIGRATION_LOCK_SQL: &str = "SELECT pg_advisory_lock($1)";

pub const MIGRATION_UNLOCK_SQL: &str = "SELECT pg_advisory_unlock($1)";

pub const DEFAULT_MAX_CONCURRENT: usize = 4;

pub const DEFAULT_RESERVED_CORE_CONNECTIONS: usize = 4;

pub type MigrationFuture<'a, T> = Pin<Box<dyn Future<Output = DbResult<T>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationStep {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationReport {
    pub plugin_id: String,
    pub applied: Vec<u32>,
    pub skipped: Vec<u32>,
}

impl MigrationReport {
    pub fn is_empty(&self) -> bool {
        self.applied.is_empty() && self.skipped.is_empty()
    }

    pub fn applied_count(&self) -> usize {
        self.applied.len()
    }

    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }
}

pub trait MigrationExecutor: Send + Sync {
    fn ensure_registry(&self) -> MigrationFuture<'_, ()>;

    fn applied_versions<'a>(&'a self, plugin_id: &'a str) -> MigrationFuture<'a, Vec<u32>>;

    fn apply_step<'a>(
        &'a self,
        plugin_id: &'a str,
        step: MigrationStep,
    ) -> MigrationFuture<'a, bool>;

    fn lock(&self) -> MigrationFuture<'_, Option<MigrationLock>> {
        Box::pin(async move { Ok(None) })
    }
}

#[derive(Debug)]
pub struct MigrationLock {
    conn: sqlx::pool::PoolConnection<sqlx::Postgres>,
    key: i64,
}

impl MigrationLock {
    pub fn key(&self) -> i64 {
        self.key
    }

    pub async fn release(mut self) -> DbResult<()> {
        sqlx::query(MIGRATION_UNLOCK_SQL)
            .bind(self.key)
            .execute(&mut *self.conn)
            .await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PgMigrationExecutor {
    pool: PgPool,
}

impl PgMigrationExecutor {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl MigrationExecutor for PgMigrationExecutor {
    fn ensure_registry(&self) -> MigrationFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query(PLUGIN_MIGRATIONS_DDL)
                .execute(&self.pool)
                .await
                .map_err(|err| DbError::Schema(format!("{err}: {PLUGIN_MIGRATIONS_DDL}")))?;
            Ok(())
        })
    }

    fn applied_versions<'a>(&'a self, plugin_id: &'a str) -> MigrationFuture<'a, Vec<u32>> {
        Box::pin(async move {
            let rows: Vec<i64> = sqlx::query_scalar(PLUGIN_MIGRATION_VERSIONS)
                .bind(plugin_id)
                .fetch_all(&self.pool)
                .await?;
            Ok(rows
                .into_iter()
                .filter_map(|version| u32::try_from(version).ok())
                .collect())
        })
    }

    fn apply_step<'a>(
        &'a self,
        plugin_id: &'a str,
        step: MigrationStep,
    ) -> MigrationFuture<'a, bool> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await?;
            for statement in split_statements(step.sql) {
                sqlx::query(statement)
                    .execute(&mut *tx)
                    .await
                    .map_err(|err| DbError::PluginMigration {
                        plugin: plugin_id.to_string(),
                        version: step.version,
                        message: err.to_string(),
                    })?;
            }
            let recorded = sqlx::query(PLUGIN_MIGRATION_RECORD)
                .bind(plugin_id)
                .bind(i64::from(step.version))
                .bind(step.name)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(recorded.rows_affected() > 0)
        })
    }

    fn lock(&self) -> MigrationFuture<'_, Option<MigrationLock>> {
        Box::pin(async move {
            let mut conn = self.pool.acquire().await?;
            sqlx::query(MIGRATION_LOCK_SQL)
                .bind(MIGRATION_LOCK_KEY)
                .execute(&mut *conn)
                .await?;
            Ok(Some(MigrationLock {
                conn,
                key: MIGRATION_LOCK_KEY,
            }))
        })
    }
}

#[derive(Debug, Default)]
pub struct InMemoryMigrationExecutor {
    applied: Mutex<BTreeMap<String, BTreeMap<u32, String>>>,
    executed: Mutex<Vec<(String, u32, &'static str)>>,
}

impl InMemoryMigrationExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_applied(&self) -> MutexGuard<'_, BTreeMap<String, BTreeMap<u32, String>>> {
        self.applied
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn executed_sql(&self) -> Vec<(String, u32, &'static str)> {
        self.executed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn executed_versions(&self, plugin_id: &str) -> Vec<u32> {
        self.executed_sql()
            .into_iter()
            .filter(|(plugin, _, _)| plugin == plugin_id)
            .map(|(_, version, _)| version)
            .collect()
    }

    pub fn stored_rows(&self, plugin_id: &str) -> Vec<(u32, String)> {
        self.lock_applied()
            .get(plugin_id)
            .map(|versions| {
                versions
                    .iter()
                    .map(|(version, name)| (*version, name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl MigrationExecutor for InMemoryMigrationExecutor {
    fn ensure_registry(&self) -> MigrationFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn applied_versions<'a>(&'a self, plugin_id: &'a str) -> MigrationFuture<'a, Vec<u32>> {
        Box::pin(async move {
            Ok(self
                .lock_applied()
                .get(plugin_id)
                .map(|versions| versions.keys().copied().collect())
                .unwrap_or_default())
        })
    }

    fn apply_step<'a>(
        &'a self,
        plugin_id: &'a str,
        step: MigrationStep,
    ) -> MigrationFuture<'a, bool> {
        Box::pin(async move {
            let inserted = self
                .lock_applied()
                .entry(plugin_id.to_string())
                .or_default()
                .insert(step.version, step.name.to_string())
                .is_none();
            if inserted {
                self.executed
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push((plugin_id.to_string(), step.version, step.name));
            }
            Ok(inserted)
        })
    }
}

pub struct MigrationRegistry {
    executor: Arc<dyn MigrationExecutor>,
    applied: Mutex<BTreeMap<String, Vec<u32>>>,
}

impl std::fmt::Debug for MigrationRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MigrationRegistry")
            .field("plugins", &self.lock_applied().len())
            .finish()
    }
}

impl MigrationRegistry {
    pub fn new(executor: Arc<dyn MigrationExecutor>) -> Self {
        Self {
            executor,
            applied: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn postgres(pool: PgPool) -> Self {
        Self::new(Arc::new(PgMigrationExecutor::new(pool)))
    }

    pub fn in_memory() -> Self {
        Self::new(Arc::new(InMemoryMigrationExecutor::new()))
    }

    pub fn executor(&self) -> &Arc<dyn MigrationExecutor> {
        &self.executor
    }

    fn lock_applied(&self) -> MutexGuard<'_, BTreeMap<String, Vec<u32>>> {
        self.applied
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn applied(&self, plugin_id: &str) -> Vec<u32> {
        self.lock_applied()
            .get(plugin_id)
            .cloned()
            .unwrap_or_default()
    }

    fn remember(&self, plugin_id: &str, versions: &[u32]) {
        let mut guard = self.lock_applied();
        let entry = guard.entry(plugin_id.to_string()).or_default();
        entry.extend(versions.iter().copied());
        entry.sort_unstable();
        entry.dedup();
    }

    pub async fn migrate(
        &self,
        plugin_id: &str,
        steps: &[MigrationStep],
    ) -> DbResult<MigrationReport> {
        if plugin_id.trim().is_empty() {
            return Err(DbError::EmptyPluginId);
        }
        let mut ordered: Vec<MigrationStep> = steps.to_vec();
        ordered.sort_by_key(|step| step.version);
        for pair in ordered.windows(2) {
            if pair[0].version == pair[1].version {
                return Err(DbError::Invalid(format!(
                    "插件 {plugin_id} 的迁移版本 {} 重复",
                    pair[0].version
                )));
            }
        }
        self.executor.ensure_registry().await?;
        let guard = self.executor.lock().await?;
        let outcome = self.run_steps(plugin_id, ordered).await;
        if let Some(guard) = guard {
            guard.release().await?;
        }
        outcome
    }

    async fn run_steps(
        &self,
        plugin_id: &str,
        ordered: Vec<MigrationStep>,
    ) -> DbResult<MigrationReport> {
        let existing: BTreeSet<u32> = self
            .executor
            .applied_versions(plugin_id)
            .await?
            .into_iter()
            .collect();
        let mut report = MigrationReport {
            plugin_id: plugin_id.to_string(),
            ..MigrationReport::default()
        };
        for step in ordered {
            if existing.contains(&step.version) {
                report.skipped.push(step.version);
                continue;
            }
            if self.executor.apply_step(plugin_id, step).await? {
                report.applied.push(step.version);
            } else {
                report.skipped.push(step.version);
            }
        }
        self.remember(plugin_id, &report.applied);
        Ok(report)
    }
}

#[derive(Debug, Default)]
pub struct PluginDbMetrics {
    in_use: AtomicUsize,
    waits: AtomicU64,
    acquisitions: AtomicU64,
    rejections: AtomicU64,
}

impl PluginDbMetrics {
    pub fn in_use(&self) -> usize {
        self.in_use.load(Ordering::Relaxed)
    }

    pub fn wait_count(&self) -> u64 {
        self.waits.load(Ordering::Relaxed)
    }

    pub fn acquisitions(&self) -> u64 {
        self.acquisitions.load(Ordering::Relaxed)
    }

    pub fn rejections(&self) -> u64 {
        self.rejections.load(Ordering::Relaxed)
    }

    fn acquired(&self) {
        self.in_use.fetch_add(1, Ordering::Relaxed);
        self.acquisitions.fetch_add(1, Ordering::Relaxed);
    }

    fn released(&self) {
        self.in_use.fetch_sub(1, Ordering::Relaxed);
    }

    fn waited(&self) {
        self.waits.fetch_add(1, Ordering::Relaxed);
    }

    fn rejected(&self) {
        self.rejections.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub struct BulkheadPermit {
    permit: Option<OwnedSemaphorePermit>,
    metrics: Arc<PluginDbMetrics>,
}

impl BulkheadPermit {
    pub fn is_held(&self) -> bool {
        self.permit.is_some()
    }
}

impl Drop for BulkheadPermit {
    fn drop(&mut self) {
        let released = self.permit.take();
        drop(released);
        self.metrics.released();
    }
}

#[derive(Debug)]
pub struct PluginDb {
    plugin_id: String,
    kv: KvStore,
    pool: PgPool,
    bulkhead: Arc<tokio::sync::Semaphore>,
    max_concurrent: usize,
    migrations: Arc<MigrationRegistry>,
    metrics: Arc<PluginDbMetrics>,
}

impl PartialEq for PluginDb {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for PluginDb {}

impl PluginDb {
    fn new(
        plugin_id: String,
        pool: PgPool,
        max_concurrent: usize,
        migrations: Arc<MigrationRegistry>,
    ) -> Self {
        let limit = max_concurrent.max(1);
        Self {
            kv: KvStore::plugin(pool.clone(), &plugin_id),
            plugin_id,
            pool,
            bulkhead: Arc::new(tokio::sync::Semaphore::new(limit)),
            max_concurrent: limit,
            migrations,
            metrics: Arc::new(PluginDbMetrics::default()),
        }
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn kv(&self) -> &KvStore {
        &self.kv
    }

    pub fn kv_owned(&self) -> KvStore {
        self.kv.clone()
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    pub fn available_permits(&self) -> usize {
        self.bulkhead.available_permits()
    }

    pub fn permits_in_use(&self) -> usize {
        self.metrics.in_use()
    }

    pub fn wait_count(&self) -> u64 {
        self.metrics.wait_count()
    }

    pub fn metrics(&self) -> &Arc<PluginDbMetrics> {
        &self.metrics
    }

    pub fn applied_versions(&self) -> Vec<u32> {
        self.migrations.applied(&self.plugin_id)
    }

    pub async fn migrate(&self, steps: &[MigrationStep]) -> DbResult<MigrationReport> {
        self.migrations.migrate(&self.plugin_id, steps).await
    }

    fn checkout(&self, permit: OwnedSemaphorePermit) -> BulkheadPermit {
        self.metrics.acquired();
        BulkheadPermit {
            permit: Some(permit),
            metrics: Arc::clone(&self.metrics),
        }
    }

    fn busy(&self) -> DbError {
        DbError::PluginBulkheadFull {
            plugin: self.plugin_id.clone(),
            limit: self.max_concurrent,
        }
    }

    pub async fn acquire(&self) -> DbResult<BulkheadPermit> {
        let semaphore = Arc::clone(&self.bulkhead);
        if semaphore.available_permits() == 0 {
            self.metrics.waited();
        }
        let permit = semaphore.acquire_owned().await.map_err(|_| self.busy())?;
        Ok(self.checkout(permit))
    }

    pub fn try_acquire(&self) -> DbResult<BulkheadPermit> {
        match Arc::clone(&self.bulkhead).try_acquire_owned() {
            Ok(permit) => Ok(self.checkout(permit)),
            Err(_) => {
                self.metrics.rejected();
                Err(self.busy())
            }
        }
    }

    pub async fn query(&self, sql: &str) -> DbResult<u64> {
        let _permit = self.acquire().await?;
        let result = sqlx::query(sql).execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    pub async fn try_query(&self, sql: &str) -> DbResult<u64> {
        let _permit = self.try_acquire()?;
        let result = sqlx::query(sql).execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    pub async fn fetch_scalar_i64(&self, sql: &str) -> DbResult<Option<i64>> {
        let _permit = self.try_acquire()?;
        let value: Option<i64> = sqlx::query_scalar(sql).fetch_optional(&self.pool).await?;
        Ok(value)
    }

    pub fn bulkhead_stats(&self) -> PluginBulkheadStats {
        PluginBulkheadStats {
            plugin_id: self.plugin_id.clone(),
            max_concurrent: self.max_concurrent,
            permits_in_use: self.metrics.in_use(),
            available: self.available_permits(),
            wait_count: self.metrics.wait_count(),
            acquisitions: self.metrics.acquisitions(),
            rejections: self.metrics.rejections(),
            migrations: self.applied_versions(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginBulkheadStats {
    pub plugin_id: String,
    pub max_concurrent: usize,
    pub permits_in_use: usize,
    pub available: usize,
    pub wait_count: u64,
    pub acquisitions: u64,
    pub rejections: u64,
    pub migrations: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryConfig {
    pub max_concurrent_per_plugin: usize,
    pub reserved_core_connections: usize,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            max_concurrent_per_plugin: DEFAULT_MAX_CONCURRENT,
            reserved_core_connections: DEFAULT_RESERVED_CORE_CONNECTIONS,
        }
    }
}

impl RegistryConfig {
    pub const fn new(max_concurrent_per_plugin: usize, reserved_core_connections: usize) -> Self {
        Self {
            max_concurrent_per_plugin,
            reserved_core_connections,
        }
    }
}

#[derive(Debug)]
pub struct PluginDbRegistry {
    pool: PgPool,
    migrations: Arc<MigrationRegistry>,
    config: RegistryConfig,
    entries: RwLock<BTreeMap<String, Arc<PluginDb>>>,
}

impl PluginDbRegistry {
    pub fn new(pool: PgPool, config: RegistryConfig) -> Self {
        let migrations = Arc::new(MigrationRegistry::postgres(pool.clone()));
        Self::with_migrations(pool, config, migrations)
    }

    pub fn with_migrations(
        pool: PgPool,
        config: RegistryConfig,
        migrations: Arc<MigrationRegistry>,
    ) -> Self {
        Self {
            pool,
            migrations,
            config,
            entries: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn config(&self) -> RegistryConfig {
        self.config
    }

    pub fn migrations(&self) -> &Arc<MigrationRegistry> {
        &self.migrations
    }

    fn lock_entries(&self) -> RwLockReadGuard<'_, BTreeMap<String, Arc<PluginDb>>> {
        self.entries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_entries_mut(&self) -> RwLockWriteGuard<'_, BTreeMap<String, Arc<PluginDb>>> {
        self.entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn plugin(&self, plugin_id: &str) -> DbResult<Arc<PluginDb>> {
        let id = plugin_id.trim();
        if id.is_empty() {
            return Err(DbError::EmptyPluginId);
        }
        if let Some(found) = self.lock_entries().get(id) {
            return Ok(Arc::clone(found));
        }
        let mut guard = self.lock_entries_mut();
        if let Some(found) = guard.get(id) {
            return Ok(Arc::clone(found));
        }
        let limit = self.limit_for(guard.len());
        let handle = Arc::new(PluginDb::new(
            id.to_string(),
            self.pool.clone(),
            limit,
            Arc::clone(&self.migrations),
        ));
        guard.insert(id.to_string(), Arc::clone(&handle));
        Ok(handle)
    }

    pub fn known_plugins(&self) -> Vec<String> {
        self.lock_entries().keys().cloned().collect()
    }

    pub fn reserved_core_connections(&self) -> usize {
        self.config.reserved_core_connections
    }

    pub fn plugin_budget(&self) -> usize {
        let max = self.pool.options().get_max_connections() as usize;
        max.saturating_sub(self.config.reserved_core_connections)
            .max(1)
    }

    pub fn total_permits(&self) -> usize {
        self.lock_entries()
            .values()
            .map(|handle| handle.max_concurrent())
            .sum()
    }

    pub fn remaining_budget(&self) -> usize {
        self.plugin_budget().saturating_sub(self.total_permits())
    }

    fn limit_for(&self, existing: usize) -> usize {
        let budget = self.plugin_budget();
        let taken = existing.saturating_mul(self.config.max_concurrent_per_plugin);
        let remaining = budget.saturating_sub(taken).max(1);
        self.config.max_concurrent_per_plugin.min(remaining).max(1)
    }

    pub fn bulkhead_stats(&self) -> Vec<PluginBulkheadStats> {
        self.lock_entries()
            .values()
            .map(|handle| handle.bulkhead_stats())
            .collect()
    }

    pub fn total_permits_in_use(&self) -> usize {
        self.lock_entries()
            .values()
            .map(|handle| handle.permits_in_use())
            .sum()
    }

    pub async fn migrate(
        &self,
        plugin_id: &str,
        steps: &[MigrationStep],
    ) -> DbResult<MigrationReport> {
        let handle = self.plugin(plugin_id)?;
        handle.migrate(steps).await
    }
}

fn split_statements(sql: &str) -> impl Iterator<Item = &str> {
    sql.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::SCOPE_PLUGIN;

    fn registry() -> PluginDbRegistry {
        PluginDbRegistry::with_migrations(
            PgPool::connect_lazy("postgres://aurorix:aurorix@127.0.0.1:55433/aurorix")
                .expect("惰性连接串"),
            RegistryConfig::default(),
            Arc::new(MigrationRegistry::in_memory()),
        )
    }

    fn steps() -> Vec<MigrationStep> {
        vec![
            MigrationStep {
                version: 3,
                name: "add_index",
                sql: "CREATE INDEX IF NOT EXISTS rating_idx ON rating(uid)",
            },
            MigrationStep {
                version: 1,
                name: "create_table",
                sql: "CREATE TABLE IF NOT EXISTS rating (uid TEXT PRIMARY KEY)",
            },
            MigrationStep {
                version: 2,
                name: "add_column",
                sql: "ALTER TABLE rating ADD COLUMN IF NOT EXISTS score BIGINT NOT NULL DEFAULT 0",
            },
        ]
    }

    #[test]
    fn migration_table_name_cannot_collide_with_core_or_legacy_tables() {
        assert_eq!(PLUGIN_MIGRATIONS_TABLE, "plugin_migrations");
        for core in crate::schema::CORE_TABLE_NAMES {
            assert_ne!(PLUGIN_MIGRATIONS_TABLE, core);
        }
        assert!(PLUGIN_MIGRATIONS_DDL.contains("CREATE TABLE IF NOT EXISTS"));
        assert!(PLUGIN_MIGRATIONS_DDL.contains("PRIMARY KEY (plugin_id, version)"));
        assert!(PLUGIN_MIGRATION_RECORD.contains("ON CONFLICT (plugin_id, version) DO NOTHING"));
    }

    #[test]
    fn migration_advisory_lock_key_is_a_stable_constant() {
        assert_eq!(MIGRATION_LOCK_KEY, 0x6175_726F_7269_7801);
        assert!(MIGRATION_LOCK_SQL.contains("pg_advisory_lock($1)"));
        assert!(MIGRATION_UNLOCK_SQL.contains("pg_advisory_unlock($1)"));
    }

    #[tokio::test]
    async fn in_memory_executor_skips_the_advisory_lock() {
        let executor = Arc::new(InMemoryMigrationExecutor::new());
        assert!(
            executor.lock().await.expect("锁操作不该失败").is_none(),
            "内存执行器没有跨进程互斥, 不该产出锁"
        );
        let migrations =
            MigrationRegistry::new(Arc::clone(&executor) as Arc<dyn MigrationExecutor>);
        let report = migrations.migrate("rating", &steps()).await.expect("迁移");
        assert_eq!(report.applied, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn migrations_apply_in_version_order_and_are_idempotent() {
        let executor = Arc::new(InMemoryMigrationExecutor::new());
        let migrations =
            MigrationRegistry::new(Arc::clone(&executor) as Arc<dyn MigrationExecutor>);
        let first = migrations
            .migrate("rating", &steps())
            .await
            .expect("首次迁移");
        assert_eq!(first.applied, vec![1, 2, 3], "必须按版本升序执行");
        assert!(first.skipped.is_empty());
        assert_eq!(executor.executed_versions("rating"), vec![1, 2, 3]);
        let second = migrations
            .migrate("rating", &steps())
            .await
            .expect("二次迁移");
        assert!(second.applied.is_empty(), "已应用的版本不该重复执行");
        assert_eq!(second.skipped, vec![1, 2, 3]);
        assert_eq!(executor.executed_versions("rating"), vec![1, 2, 3]);
        assert_eq!(migrations.applied("rating"), vec![1, 2, 3]);
        let stored = executor.stored_rows("rating");
        assert_eq!(stored.len(), 3, "迁移记录不该有重复行");
    }

    #[tokio::test]
    async fn duplicate_versions_are_rejected_before_any_write() {
        let executor = Arc::new(InMemoryMigrationExecutor::new());
        let migrations =
            MigrationRegistry::new(Arc::clone(&executor) as Arc<dyn MigrationExecutor>);
        let duplicated = vec![
            MigrationStep {
                version: 1,
                name: "a",
                sql: "SELECT 1",
            },
            MigrationStep {
                version: 1,
                name: "b",
                sql: "SELECT 2",
            },
        ];
        let err = migrations
            .migrate("rating", &duplicated)
            .await
            .expect_err("重复版本");
        assert!(err.to_string().contains("重复"), "{err}");
        assert!(executor.executed_sql().is_empty());
    }

    #[tokio::test]
    async fn empty_plugin_id_is_rejected() {
        let migrations = MigrationRegistry::in_memory();
        let err = migrations.migrate("  ", &[]).await.expect_err("空 id");
        assert!(matches!(err, DbError::EmptyPluginId));
        assert_eq!(err.to_string(), "插件 id 不能为空");
    }

    #[tokio::test]
    async fn plugin_handles_are_scoped_and_idempotent() {
        let registry = registry();
        let rating = registry.plugin("rating").expect("句柄");
        let again = registry.plugin("rating").expect("句柄");
        assert!(Arc::ptr_eq(&rating, &again), "同一个 id 只应当有一个句柄");
        assert_eq!(rating.plugin_id(), "rating");
        assert_eq!(rating.kv().scope(), SCOPE_PLUGIN);
        assert_eq!(rating.kv().namespace(), "rating");
        let arcaea = registry.plugin("arcaea").expect("句柄");
        assert_eq!(arcaea.kv().scope(), SCOPE_PLUGIN);
        assert_eq!(arcaea.kv().namespace(), "arcaea");
        assert_ne!(rating.kv().namespace(), arcaea.kv().namespace());
        assert_eq!(registry.known_plugins(), vec!["arcaea", "rating"]);
        assert!(matches!(
            registry.plugin("  ").expect_err("空 id"),
            DbError::EmptyPluginId
        ));
    }

    #[tokio::test]
    async fn bulkhead_rejects_the_n_plus_one_call_then_releases_on_drop() {
        let registry = PluginDbRegistry::with_migrations(
            PgPool::connect_lazy("postgres://aurorix:aurorix@127.0.0.1:55433/aurorix")
                .expect("惰性连接串"),
            RegistryConfig::new(2, 4),
            Arc::new(MigrationRegistry::in_memory()),
        );
        let handle = registry.plugin("rating").expect("句柄");
        assert_eq!(handle.max_concurrent(), 2);
        let first = handle.try_acquire().expect("第一个许可");
        let second = handle.try_acquire().expect("第二个许可");
        assert_eq!(handle.available_permits(), 0);
        assert_eq!(handle.permits_in_use(), 2);
        let err = handle.try_acquire().expect_err("第三个应当被拒");
        assert_eq!(
            err.to_string(),
            "插件 rating 的数据库并发已达上限 2, 请稍后重试"
        );
        assert!(matches!(err, DbError::PluginBulkheadFull { .. }));
        assert_eq!(handle.metrics().rejections(), 1);
        drop(first);
        assert_eq!(handle.available_permits(), 1);
        assert_eq!(handle.permits_in_use(), 1);
        let third = handle.try_acquire().expect("释放后可以再拿");
        drop(second);
        drop(third);
        assert_eq!(handle.available_permits(), 2);
        assert_eq!(handle.permits_in_use(), 0);
        assert_eq!(handle.metrics().acquisitions(), 3);
        let stats = handle.bulkhead_stats();
        assert_eq!(stats.plugin_id, "rating");
        assert_eq!(stats.max_concurrent, 2);
        assert_eq!(stats.available, 2);
        assert_eq!(stats.rejections, 1);
    }

    #[tokio::test]
    async fn waiting_acquire_is_counted_and_succeeds_after_release() {
        let registry = registry();
        let handle = registry.plugin("rating").expect("句柄");
        assert_eq!(handle.max_concurrent(), DEFAULT_MAX_CONCURRENT);
        let held: Vec<BulkheadPermit> = (0..DEFAULT_MAX_CONCURRENT)
            .map(|_| handle.try_acquire().expect("许可"))
            .collect();
        let waiting = {
            let handle = Arc::clone(&handle);
            tokio::spawn(async move { handle.acquire().await.map(|permit| permit.is_held()) })
        };
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(handle.wait_count(), 1, "等待应当被计数");
        drop(held);
        let acquired_after_wait = waiting.await.expect("任务").expect("拿到许可");
        assert!(acquired_after_wait, "阻塞等待后应当真的拿到许可");
    }

    #[tokio::test]
    async fn registry_reserves_core_connections_and_reports_bulkheads() {
        let registry = PluginDbRegistry::with_migrations(
            PgPool::connect_lazy("postgres://aurorix:aurorix@127.0.0.1:55433/aurorix")
                .expect("惰性连接串"),
            RegistryConfig::new(8, 14),
            Arc::new(MigrationRegistry::in_memory()),
        );
        assert_eq!(registry.reserved_core_connections(), 14);
        assert_eq!(registry.plugin_budget(), 1);
        let handle = registry.plugin("rating").expect("句柄");
        assert_eq!(handle.max_concurrent(), 1, "核心连接预留后插件只能用剩下的");
        assert_eq!(registry.total_permits(), 1);
        assert_eq!(registry.remaining_budget(), 0);
        let stats = registry.bulkhead_stats();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].plugin_id, "rating");
        assert!(stats[0].migrations.is_empty());
        assert_eq!(registry.total_permits_in_use(), 0);
    }

    #[tokio::test]
    async fn registry_migration_versions_show_up_in_bulkhead_stats() {
        let registry = registry();
        let report = registry.migrate("rating", &steps()).await.expect("迁移");
        assert_eq!(report.applied, vec![1, 2, 3]);
        let stats = registry.bulkhead_stats();
        assert_eq!(stats[0].migrations, vec![1, 2, 3]);
        assert!(report.skipped_count() == 0);
    }

    #[tokio::test]
    async fn plugin_db_equality_is_identity_based() {
        let registry = registry();
        let first = registry.plugin("rating").expect("句柄");
        let second = registry.plugin("rating").expect("句柄");
        let other = registry.plugin("arcaea").expect("句柄");
        assert_eq!(first, second);
        assert_ne!(first, other);
        assert!(format!("{:?}", first).contains("PluginDb"));
    }
}
