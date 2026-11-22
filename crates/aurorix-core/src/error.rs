use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreError {
    #[error("未知的会话场景: {0}")]
    UnknownScene(String),
    #[error("{0}")]
    Invalid(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
