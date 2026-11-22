use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GatewayError {
    #[error("获取 access token 失败: {0}")]
    Token(String),
    #[error("连接网关失败: {0}")]
    Transport(String),
    #[error("WebSocket 握手失败: {0}")]
    Handshake(String),
    #[error("握手阶段未收到 Hello 帧")]
    MissingHello,
    #[error("连接已被服务端关闭, 关闭码 {0}")]
    Closed(u16),
    #[error("心跳未在间隔内收到 ACK, 主动断线")]
    HeartbeatLost,
    #[error("读超时, 连接视为假死")]
    ReadTimeout,
    #[error("服务端要求重连")]
    Reconnect,
    #[error("会话失效, 需要重新鉴权")]
    InvalidSession,
    #[error("服务端禁止重连, 关闭码 {0}")]
    Stopped(u16),
    #[error("网关已停止")]
    Shutdown,
    #[error("帧解析失败: {0}")]
    Codec(String),
    #[error("配置错误: {0}")]
    Config(String),
}

impl GatewayError {
    pub fn is_stop(&self) -> bool {
        matches!(self, GatewayError::Stopped(_))
    }

    pub fn is_shutdown(&self) -> bool {
        matches!(self, GatewayError::Shutdown)
    }

    pub fn close_code(&self) -> Option<u16> {
        match self {
            GatewayError::Closed(code) => Some(*code),
            GatewayError::Stopped(code) => Some(*code),
            _ => None,
        }
    }
}
