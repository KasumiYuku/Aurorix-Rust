use thiserror::Error;

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("{0}")]
    Message(String),
    #[error("未知的图床: {0}")]
    UnknownProvider(String),
    #[error("读取媒体失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("请求失败: {0}")]
    Transport(String),
    #[error("响应解析失败: {0}")]
    Json(String),
    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("没有可用的图床")]
    NoProvider,
}

impl AssetError {
    pub fn message(message: impl Into<String>) -> Self {
        AssetError::Message(message.into())
    }

    pub fn status(status: u16, body: impl Into<String>) -> Self {
        AssetError::Status {
            status,
            body: body.into(),
        }
    }

    pub fn transport(message: impl Into<String>) -> Self {
        AssetError::Transport(message.into())
    }
}

pub type AssetResult<T> = Result<T, AssetError>;
