pub mod admin;
pub mod batch;
pub mod cache;
pub mod config;
pub mod config_cache;
pub mod doctor;
pub mod error;
pub mod inbox;
pub mod kv;
pub mod plugin_db;
pub mod pool;
pub mod schema;
pub mod stats;

pub use admin::{
    now_rfc3339, snapshot_key, snapshot_name, AdminFuture, AdminKv, AssetProvider, AssetStore,
    BanEntry, BanStore, PanelBody, PanelItem, PanelRecord, PanelSnapshot, PanelStore, PgKv,
    SnapshotIndexEntry, ASSET_PROVIDERS_KEY, ASSET_WHITELIST_KEY, BAN_MESSAGE_LIMIT,
    DEFAULT_BAN_MESSAGE, PANEL_KEY, PANEL_SNAPSHOT_LIMIT, PANEL_TARGET_ALL, ROSTER_KEY,
    SNAPSHOT_INDEX_KEY, SNAPSHOT_KEY_PREFIX,
};
pub use batch::{
    conv_delta, day_delta, stats_rows_of, BatchConfig, BatchFuture, BatchMerge, BatchSink, Batcher,
    BatcherStats, ConversationDelta, DurabilityPolicy, FlushReport, InboxBatch, InboxBatcher,
    InboxFlushSink, StatsBatcher, StatsFlushSink, WriteBehind, DEFAULT_CAPACITY,
    DEFAULT_FLUSH_INTERVAL, DEFAULT_MAX_BATCH, INBOX_CONVERSATIONS_UNNEST, INBOX_MESSAGES_UNNEST,
    STATS_CONVERSATIONS_UNNEST, STATS_DAILY_UNNEST,
};
pub use cache::{
    BanCache, CacheFuture, CacheStats, HotCache, RosterSource, RosterView, StalePolicy,
};
pub use config::{
    AccessConfig, AccessRule, AppConfig, Deny, Maintenance, TemplatesConfig, CONFIG_FILE,
    DEFAULT_DATABASE, DEFAULT_DATA_DIR, DEFAULT_OVERRIDE_DIR, DEFAULT_RELOAD_SECONDS,
};
pub use config_cache::{
    ConfigCache, ConfigCacheStats, CONFIG_NAMESPACE, DEFAULT_CONFIG_LIMIT_BYTES, KV_SCAN,
};
pub use doctor::{
    verify_existing_schema, SchemaReport, TableReport, TypeMismatch, COLUMNS_QUERY, INDEXES_QUERY,
    TABLES_QUERY,
};
pub use error::{DbError, DbResult};
pub use inbox::{Conversation, Direction, InboxStore, Message};
pub use kv::{KvStore, SCOPE_COMMAND, SCOPE_GLOBAL, SCOPE_PLUGIN, SCOPE_USER};
pub use plugin_db::{
    BulkheadPermit, InMemoryMigrationExecutor, MigrationExecutor, MigrationFuture, MigrationLock,
    MigrationRegistry, MigrationReport, MigrationStep, PgMigrationExecutor, PluginBulkheadStats,
    PluginDb, PluginDbMetrics, PluginDbRegistry, RegistryConfig, DEFAULT_MAX_CONCURRENT,
    DEFAULT_RESERVED_CORE_CONNECTIONS, MIGRATION_LOCK_KEY, MIGRATION_LOCK_SQL, MIGRATION_UNLOCK_SQL,
    PLUGIN_MIGRATIONS_DDL, PLUGIN_MIGRATIONS_TABLE, PLUGIN_MIGRATION_RECORD,
    PLUGIN_MIGRATION_VERSIONS,
};
pub use pool::Db;
pub use schema::{
    core_table, ColumnSpec, TableSpec, CORE_TABLES, CORE_TABLE_NAMES, INBOX_CONVERSATIONS_COLUMNS,
    INBOX_MESSAGES_COLUMNS, KV_DATA_COLUMNS, STATS_CONVERSATIONS_COLUMNS, STATS_DAILY_COLUMNS,
    STATS_MIGRATIONS_COLUMNS,
};
pub use stats::{
    ConvDelta, ConversationRow, Counts, DayDelta, DayStat, LifeTotals, StatsBatch, StatsStore,
    StatsView,
};
