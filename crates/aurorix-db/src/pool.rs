use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::admin::{AssetStore, BanStore, PanelStore};
use crate::batch::{
    BatchConfig, InboxBatcher, InboxFlushSink, StatsBatcher, StatsFlushSink, WriteBehind,
};
use crate::cache::BanCache;
use crate::config::AppConfig;
use crate::config_cache::ConfigCache;
use crate::doctor::{verify_existing_schema, SchemaReport};
use crate::error::{DbError, DbResult};
use crate::inbox::InboxStore;
use crate::kv::KvStore;
use crate::plugin_db::{PluginDbRegistry, RegistryConfig};
use crate::schema;
use crate::stats::StatsStore;

pub const MIN_CONNECTIONS: u32 = 2;

const MAX_CONNECTIONS: u32 = 16;

pub const MAX_IDLE_CONNECTIONS: u32 = 8;

pub const MAX_LIFETIME_SECONDS: u64 = 3600;

pub const ACQUIRE_TIMEOUT_SECONDS: u64 = 30;

pub const IDLE_TIMEOUT_SECONDS: u64 = 600;

#[derive(Debug, Clone)]
pub struct Db {
    pool: PgPool,
    config: Arc<ConfigCache>,
}

impl Db {
    pub async fn connect(dsn: &str) -> DbResult<Self> {
        let max = aurorix_core::HardwareProfile::cached().db_connections();
        Self::connect_with(dsn, max).await
    }

    pub async fn connect_with(dsn: &str, max_connections: u32) -> DbResult<Self> {
        if dsn.is_empty() {
            return Err(DbError::EmptyDsn);
        }
        let max = max_connections.clamp(MIN_CONNECTIONS, MAX_CONNECTIONS);
        let pool = PgPoolOptions::new()
            .max_connections(max)
            .min_connections(MAX_IDLE_CONNECTIONS.min(max))
            .max_lifetime(Duration::from_secs(MAX_LIFETIME_SECONDS))
            .acquire_timeout(Duration::from_secs(ACQUIRE_TIMEOUT_SECONDS))
            .idle_timeout(Duration::from_secs(IDLE_TIMEOUT_SECONDS))
            .connect(dsn)
            .await?;
        let db = Self::assemble(pool);
        db.ensure_schema().await?;
        Ok(db)
    }

    pub async fn from_pool(pool: PgPool) -> DbResult<Self> {
        let db = Self::assemble(pool);
        db.ensure_schema().await?;
        Ok(db)
    }

    fn assemble(pool: PgPool) -> Self {
        let config = Arc::new(ConfigCache::new(KvStore::global(pool.clone())));
        Self { pool, config }
    }

    pub fn config_cache(&self) -> Arc<ConfigCache> {
        Arc::clone(&self.config)
    }

    pub async fn from_config(config: &AppConfig) -> DbResult<Self> {
        Self::connect(config.database_url()).await
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn clone_pool(&self) -> PgPool {
        self.pool.clone()
    }

    pub async fn ensure_schema(&self) -> DbResult<()> {
        schema::ensure_schema(&self.pool).await
    }

    pub fn kv_global(&self) -> KvStore {
        KvStore::global(self.pool.clone())
    }

    pub fn kv_plugin(&self, plugin_id: &str) -> KvStore {
        KvStore::plugin(self.pool.clone(), plugin_id)
    }

    pub fn inbox(&self) -> InboxStore {
        InboxStore::new(self.pool.clone())
    }

    pub fn stats(&self) -> StatsStore {
        StatsStore::new(self.pool.clone())
    }

    pub fn bans(&self) -> BanStore {
        BanStore::from_kv(self.kv_global())
    }

    pub fn panels(&self) -> PanelStore {
        PanelStore::from_kv(self.kv_global())
    }

    pub fn assets(&self) -> AssetStore {
        AssetStore::from_kv(self.kv_global())
    }

    pub fn ban_cache(&self) -> BanCache {
        BanCache::from_store(self.bans())
    }

    pub fn stats_batcher(&self, config: BatchConfig) -> Arc<StatsBatcher> {
        Arc::new(crate::batch::Batcher::new(
            Arc::new(StatsFlushSink::new(self.pool.clone())),
            config,
        ))
    }

    pub fn inbox_batcher(&self, config: BatchConfig) -> Arc<InboxBatcher> {
        Arc::new(crate::batch::Batcher::new(
            Arc::new(InboxFlushSink::new(self.pool.clone())),
            config,
        ))
    }

    pub fn write_behind(&self, config: BatchConfig) -> WriteBehind {
        WriteBehind {
            stats: self.stats_batcher(config),
            inbox: self.inbox_batcher(config),
        }
    }

    pub fn plugin_registry(&self, config: RegistryConfig) -> PluginDbRegistry {
        PluginDbRegistry::new(self.pool.clone(), config)
    }

    pub async fn verify_schema(&self) -> DbResult<SchemaReport> {
        verify_existing_schema(&self.pool).await
    }

    pub async fn close(self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_dsn_is_rejected_before_touching_network() {
        let err = Db::connect("").await.unwrap_err();
        assert!(matches!(err, DbError::EmptyDsn));
        assert_eq!(err.to_string(), "storage: 数据库连接串为空");
    }

    #[test]
    fn pool_limits_match_go() {
        assert_eq!(MAX_CONNECTIONS, 16);
        assert_eq!(MAX_IDLE_CONNECTIONS, 8);
        assert_eq!(MAX_LIFETIME_SECONDS, 3600);
        assert_eq!(ACQUIRE_TIMEOUT_SECONDS, 30);
        assert_eq!(IDLE_TIMEOUT_SECONDS, 600);
    }
}
