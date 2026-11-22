use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LogError {
    #[error("未知日志级别 \"{0}\" (可选 debug/info/warn/error)")]
    UnknownLevel(String),
    #[error("日志广播通道已关闭")]
    Closed,
    #[error("日志文件操作失败: {0}")]
    Io(String),
}
