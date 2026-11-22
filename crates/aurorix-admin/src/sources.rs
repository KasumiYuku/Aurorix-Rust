use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use aurorix_db::{DbError, StatsView};
use aurorix_log::{LogRecord, Logger, RingFilter};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::error::ApiError;

pub type SourceFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Server(String),
}

impl SourceError {
    pub fn status(&self) -> StatusCode {
        match self {
            SourceError::BadRequest(_) => StatusCode::BAD_REQUEST,
            SourceError::NotFound(_) => StatusCode::NOT_FOUND,
            SourceError::Conflict(_) => StatusCode::CONFLICT,
            SourceError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            SourceError::Server(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            SourceError::BadRequest(message)
            | SourceError::NotFound(message)
            | SourceError::Conflict(message)
            | SourceError::Unavailable(message)
            | SourceError::Server(message) => message,
        }
    }

    pub fn from_db(err: DbError) -> Self {
        match err {
            DbError::Invalid(message) => SourceError::BadRequest(message),
            other => SourceError::Server(format!("存储失败: {other}")),
        }
    }

    pub fn from_message(message: String) -> Self {
        SourceError::BadRequest(message)
    }
}

impl From<SourceError> for ApiError {
    fn from(err: SourceError) -> Self {
        match err {
            SourceError::BadRequest(message) => ApiError::BadRequest(message),
            SourceError::NotFound(message) => ApiError::NotFound(message),
            SourceError::Conflict(message) => ApiError::Conflict(message),
            SourceError::Unavailable(message) => ApiError::Unavailable(message),
            SourceError::Server(message) => ApiError::Server(message),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayStatus {
    pub connected: bool,
    pub session_id: String,
    pub seq: i64,
    pub heartbeat_ack: bool,
    pub since_ms: i64,
    pub bot_id: String,
    pub bot_name: String,
    pub shard_id: u32,
    pub shards: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub plugins: u64,
    pub commands: u64,
    pub jobs: u64,
    pub templates: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusView {
    pub uptime_seconds: u64,
    pub version: String,
    pub protocol: String,
    pub port: u16,
    pub tasks: u64,
    pub gateway: GatewayStatus,
    pub counts: Counts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogOverview {
    pub total: u64,
    pub errors: u64,
    pub levels: BTreeMap<String, u64>,
    pub scopes: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub version: String,
    pub enabled: bool,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub fields: Vec<serde_json::Value>,
    #[serde(default = "empty_object")]
    pub values: serde_json::Value,
    #[serde(default = "default_access")]
    pub access: serde_json::Value,
}

impl Default for PluginInfo {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            version: String::new(),
            enabled: false,
            commands: Vec::new(),
            fields: Vec::new(),
            values: empty_object(),
            access: default_access(),
        }
    }
}

pub fn empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

pub fn default_access() -> serde_json::Value {
    serde_json::json!({
        "default": {
            "mode": "whitelist",
            "users": [],
            "groups": [],
            "maintenance": { "enabled": false },
            "deny": {},
        },
        "commands": {},
        "disabled": false,
        "maintenance": { "enabled": false },
        "deny": {},
    })
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PluginError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Server(String),
}

pub trait StatusSource: Send + Sync {
    fn snapshot(&self) -> StatusView;
}

pub trait StatsSource: Send + Sync {
    fn view(&self) -> StatsView;

    fn view_async(&self) -> SourceFuture<'_, StatsView> {
        Box::pin(async move { self.view() })
    }
}

pub trait LogSource: Send + Sync {
    fn recent(&self, limit: usize) -> Vec<LogRecord>;
    fn subscribe(&self) -> broadcast::Receiver<LogRecord>;
    fn overview(&self) -> LogOverview;

    fn query(&self, filter: &RingFilter) -> Vec<LogRecord> {
        let limit = filter.effective_limit();
        self.recent(limit)
            .into_iter()
            .filter(|record| filter.matches(record))
            .take(limit)
            .collect()
    }
}

pub trait PluginSource: Send + Sync {
    fn plugins(&self) -> Vec<PluginInfo>;

    fn plugin(&self, id: &str) -> Option<PluginInfo> {
        self.plugins().into_iter().find(|plugin| plugin.id == id)
    }

    fn save_config(
        &self,
        _id: &str,
        _values: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), PluginError> {
        Ok(())
    }

    fn save_access(&self, _id: &str, _access: serde_json::Value) -> Result<(), PluginError> {
        Ok(())
    }
}

pub trait ConfigListener: Send + Sync {
    fn applied(&self, config: &aurorix_db::AppConfig);
}


impl LogSource for Logger {
    fn recent(&self, limit: usize) -> Vec<LogRecord> {
        Logger::recent(self, limit)
    }
    fn query(&self, filter: &RingFilter) -> Vec<LogRecord> {
        Logger::query(self, filter)
    }

    fn subscribe(&self) -> broadcast::Receiver<LogRecord> {
        Logger::subscribe(self)
    }

    fn overview(&self) -> LogOverview {
        let stats = Logger::stats(self);
        let summary = Logger::summary(self);
        LogOverview {
            total: stats.total,
            errors: Logger::errors(self),
            levels: summary.levels,
            scopes: summary.scopes,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyStatus;

impl StatusSource for EmptyStatus {
    fn snapshot(&self) -> StatusView {
        StatusView::default()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyStats;

impl StatsSource for EmptyStats {
    fn view(&self) -> StatsView {
        StatsView::default()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyPlugins;

impl PluginSource for EmptyPlugins {
    fn plugins(&self) -> Vec<PluginInfo> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyLogs;

impl LogSource for EmptyLogs {
    fn recent(&self, _limit: usize) -> Vec<LogRecord> {
        Vec::new()
    }

    fn subscribe(&self) -> broadcast::Receiver<LogRecord> {
        aurorix_log::global().subscribe()
    }

    fn overview(&self) -> LogOverview {
        LogOverview::default()
    }
}
