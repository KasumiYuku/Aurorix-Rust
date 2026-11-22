use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    #[error("不认识的操作码 {0}")]
    UnknownOp(i32),
    #[error("不认识的事件名 {0:?}")]
    UnknownEvent(String),
    #[error("不认识的消息类型 {0}")]
    UnknownMessageKind(u8),
    #[error("场景 {0} 不支持发送消息")]
    UnsupportedScene(&'static str),
    #[error("JSON 解析失败: {0}")]
    Json(String),
    #[error("重试次数已用尽, 上限 {attempts} 次")]
    RetryExhausted { attempts: u32 },
}

pub type ProtocolResult<T> = Result<T, ProtocolError>;
