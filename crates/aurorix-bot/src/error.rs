use thiserror::Error;

pub const FALLBACK_HINT: &str =
    "请在 Bot::builder 上用 with_* 注入该依赖, 或显式调用 allow_fallbacks() 接受内存兜底";

#[derive(Debug, Error)]
pub enum BotError {
    #[error("配置不合法: {0}")]
    Config(String),
    #[error("配置缺失: {field} 为空; {hint}")]
    ConfigMissing {
        field: &'static str,
        hint: &'static str,
    },
    #[error("缺少必需依赖 {name}: {detail}; {FALLBACK_HINT}")]
    MissingDependency { name: &'static str, detail: String },
    #[error("网关装配失败: {0}")]
    Gateway(String),
    #[error("管理台启动失败: {0}")]
    Admin(String),
    #[error("已经启动过, 不能重复启动: {0}")]
    AlreadyStarted(String),
    #[error("单实例锁获取失败: {0}")]
    InstanceLock(String),
    #[error("停机超时: {remaining} 个后台任务在 {timeout_ms}ms 内没有退出, 已强制中止")]
    ShutdownTimeout { remaining: usize, timeout_ms: u64 },
}

impl BotError {
    pub fn missing(name: &'static str, detail: impl Into<String>) -> Self {
        Self::MissingDependency {
            name,
            detail: detail.into(),
        }
    }

    pub fn hint_text() -> &'static str {
        FALLBACK_HINT
    }
}
