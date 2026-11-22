use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("storage: 数据库连接串为空")]
    EmptyDsn,
    #[error("invalid storage namespace")]
    InvalidNamespace,
    #[error("storage key cannot be empty")]
    EmptyKey,
    #[error("storage target cannot be nil")]
    EmptyTarget,
    #[error("marshal storage value: {0}")]
    Encode(String),
    #[error("storage: 写回缓冲区已满, 已挂起 {pending} 行, 上限 {capacity} 行")]
    BatchBufferFull { pending: usize, capacity: usize },
    #[error("插件 id 不能为空")]
    EmptyPluginId,
    #[error("插件 {plugin} 的数据库并发已达上限 {limit}, 请稍后重试")]
    PluginBulkheadFull { plugin: String, limit: usize },
    #[error("插件 {plugin} 的数据库迁移失败 (版本 {version}): {message}")]
    PluginMigration {
        plugin: String,
        version: u32,
        message: String,
    },
    #[error("unmarshal storage value: {0}")]
    Decode(String),
    #[error("键 {0:?} 存的不是整数, 无法自增: {1}")]
    NotAnInteger(String, String),
    #[error("initialize schema: {0}")]
    Schema(String),
    #[error("postgres: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

pub type DbResult<T> = Result<T, DbError>;
