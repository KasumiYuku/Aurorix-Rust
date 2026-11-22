use std::fmt;

use aurorix_core::Scene;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    Unknown,
    RateLimit,
    Auth,
    Permission,
    NotFound,
    Audit,
    Server,
    Param,
    PassiveExpired,
}

impl ErrorKind {
    pub const ALL: [ErrorKind; 9] = [
        ErrorKind::Unknown,
        ErrorKind::RateLimit,
        ErrorKind::Auth,
        ErrorKind::Permission,
        ErrorKind::NotFound,
        ErrorKind::Audit,
        ErrorKind::Server,
        ErrorKind::Param,
        ErrorKind::PassiveExpired,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            ErrorKind::Unknown => "未分类",
            ErrorKind::RateLimit => "限流",
            ErrorKind::Auth => "认证类",
            ErrorKind::Permission => "权限类",
            ErrorKind::NotFound => "未找到",
            ErrorKind::Audit => "审核中",
            ErrorKind::Server => "可重试服务器类",
            ErrorKind::Param => "参数类",
            ErrorKind::PassiveExpired => "被动过期",
        }
    }

    pub const fn classified(self) -> bool {
        !matches!(self, ErrorKind::Unknown)
    }

    pub const fn server_side(self) -> bool {
        matches!(self, ErrorKind::Server)
    }

    pub const fn passive_expired(self) -> bool {
        matches!(self, ErrorKind::PassiveExpired)
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

pub fn describe(code: i32) -> &'static str {
    match code {
        100016 => "secret 输入错误",
        10004 => "appid 输入错误",
        100007 => "机器人被封禁或不存在",
        100017 => "接口调用超过频率限制",
        11300 => "仅私域机器人可用（link type check failed）",
        11700 => "机器人已取消该能力",
        11253 => "按钮回调权限不足",
        630006 => "header appid 获取失败",
        304023 => "消息正在审核中（audit）",
        304004 => "无权限使用该 ARK 模板",
        304036 => "无 Markdown 模板权限",
        304061 => "消息内容无效",
        304064 => "订阅消息未授权",
        304080 => "文件信息无效",
        304103 => "消息 ID 已过期，不能回复",
        305007 => "键盘样式参数错误",
        340069 => "消息类型无效",
        22006 => "消息类型与内容不匹配",
        40034004 => "富媒体信息转存失败，请重试",
        40034005 => "回复消息 msg_id 已过期",
        40034006 => "消息内容违规",
        40034008 => "markdown 参数有空值",
        40034009 => "markdown 参数有换行符",
        40034010 => "模板参数不能含 markdown 语法",
        40034011 => "无效的 markdown 内容",
        40034024 => "msg_id 无效或越权",
        40034025 => "event_id 无效（事件尚未落库）",
        40034026 => "event_id 已过期",
        40034027 => "该事件不支持被动回复",
        40034028 => "消息内容包含平台不允许的链接",
        40034029 => "内联键盘行/列超限",
        40034031 => "消息已过期，不能回复",
        40034100 => "主动消息超过频控限制",
        40034101 => "机器人非群成员",
        40034105 => "主动消息发送失败，无权限",
        40034106 => "消息不支持该指令类型",
        40034108 => "指令参数长度超限",
        40034109 => "指令参数解析失败",
        40034124 => "markdown 消息参数错误",
        40034127 => "无 markdown 模板权限",
        40034128 => "被动回复时间或次数超限",
        40054002 => "机器人被禁言",
        40054003 => "机器人不是群成员",
        40054005 => "消息被去重（同一 msg_id + msg_seq 重复发送）",
        40054007 => "消息长度超限",
        40054010 => "不允许发送 URL",
        40054016 => "机器人已下线",
        40064004 => "已超出消息撤回时限",
        50055001 => "消息发送异常，请稍后重试",
        50055006 => "ARK 消息发送异常，请稍后重试",
        _ => "未知错误",
    }
}

pub fn classify(code: i32) -> ErrorKind {
    match code {
        100016 | 10004 | 100007 => ErrorKind::Auth,
        100017 | 40034100 => ErrorKind::RateLimit,
        11300 | 11700 | 304004 | 304036 | 304064 | 40034101 | 40034105 | 40034127 | 40054002
        | 40054003 | 40054016 => ErrorKind::Permission,
        40034024 => ErrorKind::NotFound,
        304023 => ErrorKind::Audit,
        40034004 | 50055001 | 50055006 => ErrorKind::Server,
        22006 | 304061 | 304080 | 305007 | 340069 | 40034006 | 40034008 | 40034009 | 40034010
        | 40034011 | 40034025 | 40034028 | 40034029 | 40034106 | 40034108 | 40034109 | 40034124
        | 40054007 | 40054010 | 40064004 => ErrorKind::Param,
        304103 | 40034005 | 40034026 | 40034027 | 40034031 | 40034128 | 40054005 => {
            ErrorKind::PassiveExpired
        }
        _ => ErrorKind::Unknown,
    }
}

pub fn is_retryable(code: i32, extra: &[i32]) -> bool {
    match classify(code) {
        ErrorKind::Server => true,
        ErrorKind::Unknown => extra.contains(&code),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusinessError {
    pub code: i32,
    pub message: String,
    pub kind: ErrorKind,
}

impl BusinessError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            kind: classify(code),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn describe(&self) -> &'static str {
        describe(self.code)
    }

    pub fn is_passive_expired(&self) -> bool {
        self.kind.passive_expired()
    }

    pub fn is_retryable(&self, extra: &[i32]) -> bool {
        is_retryable(self.code, extra)
    }
}

impl fmt::Display for BusinessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.describe(), self.message)
    }
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("{0}")]
    Platform(BusinessError),
    #[error("接口调用失败: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("接口返回 {status}: {body}")]
    Status { status: u16, body: String },
    #[error("响应解析失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{scene}场景不支持{what}")]
    Unsupported { scene: Scene, what: &'static str },
    #[error("{0}")]
    InvalidTarget(&'static str),
    #[error("被动回复序号预算已用尽（上限 {limit} 条）")]
    ReplyBudgetExhausted { limit: u32 },
    #[error("消息审核未通过")]
    AuditRejected,
    #[error("消息审核等待超时")]
    AuditTimeout,
    #[error("请求被限流: key={key} 上限={limit} 窗口={window_ms}ms")]
    RateLimited {
        key: String,
        limit: u32,
        window_ms: u64,
    },
    #[error("媒体上传失败: {0}")]
    Upload(String),
    #[error("配置错误: {0}")]
    Config(String),
    #[error("令牌获取失败: {0}")]
    Token(String),
    #[error("{last}（被动回复先失败: {first}）")]
    Fallback { first: String, last: Box<ApiError> },
    #[error("键盘序列化失败: {0}")]
    Keyboard(String),
}

impl ApiError {
    pub fn kind(&self) -> Option<ErrorKind> {
        match self {
            ApiError::Platform(err) => Some(err.kind()),
            ApiError::Fallback { last, .. } => last.kind(),
            _ => None,
        }
    }

    pub fn code(&self) -> Option<i32> {
        match self {
            ApiError::Platform(err) => Some(err.code),
            ApiError::Fallback { last, .. } => last.code(),
            _ => None,
        }
    }

    pub fn is_passive_expired(&self) -> bool {
        match self {
            ApiError::Platform(err) => err.is_passive_expired(),
            ApiError::Fallback { last, .. } => last.is_passive_expired(),
            _ => false,
        }
    }

    pub fn is_retryable(&self, extra: &[i32]) -> bool {
        match self {
            ApiError::Platform(err) => err.is_retryable(extra),
            ApiError::Fallback { last, .. } => last.is_retryable(extra),
            _ => false,
        }
    }

    pub fn upload(message: impl Into<String>) -> Self {
        ApiError::Upload(message.into())
    }
}

impl PartialEq for ApiError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ApiError::Platform(a), ApiError::Platform(b)) => a == b,
            (
                ApiError::Status {
                    status: a,
                    body: ab,
                },
                ApiError::Status {
                    status: b,
                    body: bb,
                },
            ) => a == b && ab == bb,
            (
                ApiError::Unsupported { scene: a, what: aw },
                ApiError::Unsupported { scene: b, what: bw },
            ) => a == b && aw == bw,
            (ApiError::InvalidTarget(a), ApiError::InvalidTarget(b)) => a == b,
            (
                ApiError::ReplyBudgetExhausted { limit: a },
                ApiError::ReplyBudgetExhausted { limit: b },
            ) => a == b,
            (ApiError::AuditRejected, ApiError::AuditRejected) => true,
            (ApiError::AuditTimeout, ApiError::AuditTimeout) => true,
            (
                ApiError::RateLimited {
                    key: ak,
                    limit: al,
                    window_ms: aw,
                },
                ApiError::RateLimited {
                    key: bk,
                    limit: bl,
                    window_ms: bw,
                },
            ) => ak == bk && al == bl && aw == bw,
            (ApiError::Upload(a), ApiError::Upload(b)) => a == b,
            (ApiError::Config(a), ApiError::Config(b)) => a == b,
            (ApiError::Token(a), ApiError::Token(b)) => a == b,
            (ApiError::Keyboard(a), ApiError::Keyboard(b)) => a == b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_required_kinds_exist() {
        assert_eq!(ErrorKind::Server.label(), "可重试服务器类");
        assert_eq!(ErrorKind::PassiveExpired.label(), "被动过期");
        assert_eq!(ErrorKind::Audit.label(), "审核中");
        assert_eq!(ErrorKind::Param.label(), "参数类");
        assert_eq!(ErrorKind::Auth.label(), "认证类");
        assert_eq!(ErrorKind::ALL.len(), 9);
        assert!(ErrorKind::Server.server_side());
        assert!(!ErrorKind::Param.server_side(), "参数类不该算服务器类");
        assert!(ErrorKind::PassiveExpired.passive_expired());
        assert!(!ErrorKind::Unknown.classified(), "未分类不该算已分类");
        assert!(ErrorKind::Audit.classified());
    }

    #[test]
    fn classification_table_matches_go() {
        assert_eq!(classify(100016), ErrorKind::Auth);
        assert_eq!(classify(10004), ErrorKind::Auth);
        assert_eq!(classify(100007), ErrorKind::Auth);
        assert_eq!(classify(100017), ErrorKind::RateLimit);
        assert_eq!(classify(40034100), ErrorKind::RateLimit);
        assert_eq!(classify(40034127), ErrorKind::Permission);
        assert_eq!(classify(40034024), ErrorKind::NotFound);
        assert_eq!(classify(304023), ErrorKind::Audit);
        assert_eq!(classify(40034004), ErrorKind::Server);
        assert_eq!(classify(50055001), ErrorKind::Server);
        assert_eq!(classify(50055006), ErrorKind::Server);
        assert_eq!(classify(304061), ErrorKind::Param);
        assert_eq!(
            classify(40054005),
            ErrorKind::PassiveExpired,
            "同一 msg_id 的被动回复被去重说明锚点已不可再用, 必须走主动降级"
        );
        assert_eq!(classify(304103), ErrorKind::PassiveExpired);
        assert_eq!(classify(40034031), ErrorKind::PassiveExpired);
        assert_eq!(classify(11253), ErrorKind::Unknown, "11253 未分类");
        assert_eq!(classify(630006), ErrorKind::Unknown, "630006 未分类");
        assert_eq!(classify(424242), ErrorKind::Unknown);
    }

    #[test]
    fn retry_needs_server_kind_or_listed_code() {
        let extra = [11253, 630006];
        assert!(is_retryable(50055001, &[]), "服务器类无需配置即可重试");
        assert!(is_retryable(40034004, &[]));
        assert!(
            is_retryable(11253, &extra),
            "未分类码在 retry_when 里要重试"
        );
        assert!(is_retryable(630006, &extra));
        assert!(
            !is_retryable(11253, &[]),
            "未分类码不在 retry_when 里不重试"
        );
        assert!(
            !is_retryable(304103, &extra),
            "被动过期不在 retry_when 里也不重试"
        );
        assert!(!is_retryable(40034025, &extra), "参数类码列进配置也不重试");
        assert!(!is_retryable(424242, &extra));
    }

    #[test]
    fn describe_has_chinese_hint() {
        assert_eq!(describe(304103), "消息 ID 已过期，不能回复");
        assert_eq!(describe(40034031), "消息已过期，不能回复");
        assert_eq!(describe(999999), "未知错误");
    }

    #[test]
    fn business_error_carries_kind() {
        let err = BusinessError::new(40034005, "回复消息 msg_id 已过期");
        assert_eq!(err.kind(), ErrorKind::PassiveExpired);
        assert!(err.is_passive_expired());
        assert!(err.to_string().starts_with("回复消息 msg_id 已过期"));
        let plain = BusinessError::new(12, "x");
        assert_eq!(plain.kind(), ErrorKind::Unknown);
        assert!(!plain.is_retryable(&[]));
    }

    #[test]
    fn api_error_helpers_forward_platform_codes() {
        let err = ApiError::Platform(BusinessError::new(40034031, "过期"));
        assert_eq!(err.code(), Some(40034031));
        assert_eq!(err.kind(), Some(ErrorKind::PassiveExpired));
        assert!(err.is_passive_expired());
        let io = ApiError::InvalidTarget("会话目标为空");
        assert_eq!(io.code(), None);
        assert!(!io.is_passive_expired());
        assert!(!io.is_retryable(&[]));
        assert_eq!(ApiError::AuditTimeout, ApiError::AuditTimeout);
        assert_ne!(ApiError::AuditTimeout, ApiError::AuditRejected);
    }

    #[test]
    fn api_error_equality_is_field_wise() {
        let one = ApiError::Platform(BusinessError::new(40034031, "过期"));
        let same = ApiError::Platform(BusinessError::new(40034031, "过期"));
        let other = ApiError::Platform(BusinessError::new(40034005, "过期"));
        assert_eq!(one, same, "同码同文案要相等");
        assert_ne!(one, other, "码不同不能相等");
        assert_eq!(
            ApiError::Status {
                status: 200,
                body: String::from("x")
            },
            ApiError::Status {
                status: 200,
                body: String::from("x")
            }
        );
        assert_ne!(
            ApiError::Status {
                status: 200,
                body: String::from("x")
            },
            ApiError::Status {
                status: 200,
                body: String::from("y")
            },
            "状态码相同正文不同不能相等"
        );
        let limited = |limit: u32| ApiError::RateLimited {
            key: String::from("push"),
            limit,
            window_ms: 60000,
        };
        assert_eq!(limited(20), limited(20));
        assert_ne!(limited(20), limited(21), "上限不同不能相等");
        assert_eq!(
            ApiError::Unsupported {
                scene: Scene::Channel,
                what: "富媒体上传"
            },
            ApiError::Unsupported {
                scene: Scene::Channel,
                what: "富媒体上传"
            }
        );
        assert_ne!(
            ApiError::Unsupported {
                scene: Scene::Channel,
                what: "富媒体上传"
            },
            ApiError::Unsupported {
                scene: Scene::Channel,
                what: "官方图床"
            }
        );
    }
}
