use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TemplateError {
    #[error("key {key} has unsupported type: {type_name}")]
    UnsupportedArgType { key: String, type_name: String },
    #[error("{kind} 模板 {id} 不存在(已查 {scope})")]
    TemplateNotFound {
        kind: &'static str,
        id: String,
        scope: String,
    },
    #[error("Lost args: {0}")]
    LostArgs(String),
    #[error("invalid each tag: {0}")]
    InvalidEachTag(String),
    #[error("invalid each tag: missing key")]
    EachMissingKey,
    #[error("each {0}: missing {{{{/each}}}}")]
    EachMissingEnd(String),
    #[error("each {0}: nested each not supported")]
    EachNested(String),
    #[error("each {key}: arg must be []any, got {type_name}")]
    EachArgType { key: String, type_name: String },
    #[error("each {key}: item {index} must be map[string]any, got {type_name}")]
    EachItemType {
        key: String,
        index: usize,
        type_name: String,
    },
    #[error("inline expects 2 or 3 args: label command [reference]")]
    InlineArgCount,
    #[error("inline arg {0} not found")]
    InlineArgNotFound(String),
    #[error("inline label must not be empty")]
    InlineEmptyLabel,
    #[error("inline command must not be empty")]
    InlineEmptyCommand,
    #[error("inline quoted arg has dangling escape")]
    InlineDanglingEscape,
    #[error("inline quoted arg is not closed")]
    InlineQuotedUnclosed,
    #[error("not an inline call: {0}")]
    NotInlineCall(String),
    #[error("qqcard 要写款式, 例如: {{{{qqcard 2 text1=标题}}}}")]
    QQCardMissingStyle,
    #[error("qqcard 不认识第 {style:?} 款卡片, 可用的是 {available}")]
    QQCardUnknownStyle { style: String, available: String },
    #[error("qqcard {style} 没有 {key:?} 这个参数, 可用的是 {available}")]
    QQCardUnknownParam {
        style: String,
        key: String,
        available: String,
    },
    #[error("qqcard 的 {key}={value}: 找不到 {value:?} 这个参数")]
    QQCardArgNotFound { key: String, value: String },
    #[error("qqcard: {0}")]
    QQCardMalformed(String),
    #[error("not a qqcard call: {0}")]
    NotQQCardCall(String),
    #[error("参数要写成 名字=值, 这里读不懂: {0:?}")]
    NamedArgMissingEquals(String),
    #[error("有个参数没写名字")]
    NamedArgMissingName,
    #[error("有个值的引号没闭合")]
    NamedArgUnclosedQuote,
    #[error("缺少开头的关键字")]
    MissingLeadingToken,
    #[error("模板错误: {0}")]
    Io(String),
}

pub type TemplateResult<T> = Result<T, TemplateError>;
