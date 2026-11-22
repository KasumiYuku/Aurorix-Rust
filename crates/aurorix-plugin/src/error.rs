use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PluginError {
    #[error("回复发送失败: {0}")]
    Reply(String),
    #[error("指令执行失败: {0}")]
    Handler(String),
    #[error("参数不合法: {0}")]
    Invalid(String),
    #[error("⏳ 触发太频繁了，{seconds} 秒后再试")]
    RateLimited { seconds: u64 },
}

impl PluginError {
    pub fn handler(message: impl Into<String>) -> Self {
        Self::Handler(message.into())
    }

    pub fn reply(message: impl Into<String>) -> Self {
        Self::Reply(message.into())
    }
}

pub type PluginResult<T> = Result<T, PluginError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_text_is_chinese_prefix() {
        assert_eq!(
            PluginError::handler("找不到处理器").to_string(),
            "指令执行失败: 找不到处理器"
        );
        assert_eq!(
            PluginError::reply("网络断了").to_string(),
            "回复发送失败: 网络断了"
        );
        assert_eq!(
            PluginError::Invalid("空名字".to_string()).to_string(),
            "参数不合法: 空名字"
        );
        assert_eq!(
            PluginError::RateLimited { seconds: 5 }.to_string(),
            "⏳ 触发太频繁了，5 秒后再试"
        );
    }
}
