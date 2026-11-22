use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ButtonError {
    #[error("行号需在 0~4 之间，收到 {0}")]
    RowOutOfRange(i32),
    #[error("The row {0} is full, can't append new button")]
    RowFull(i32),
    #[error("Rows must less than 5 lines, but this keyboard has {0}")]
    TooManyRows(usize),
    #[error("Buttons in one row must less than 5, but row {row} has {count}")]
    TooManyButtons { row: usize, count: usize },
    #[error("Button {0} need CallbackData when ActionType is Callback")]
    CallbackWithoutData(String),
    #[error("Button {0} need Msg when ActionType is Command")]
    CommandWithoutMsg(String),
    #[error("Button {0} need Url when ActionType is Link")]
    LinkWithoutUrl(String),
    #[error("Button {id} define a invailed actionType: {code}")]
    InvalidActionType { id: String, code: i32 },
    #[error("底部按钮配置: {0}")]
    Footer(String),
    #[error("JSON 序列化失败: {0}")]
    Json(String),
}

pub type ButtonResult<T> = Result<T, ButtonError>;
