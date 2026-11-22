use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SdkError {
    #[error("模板渲染失败: {0}")]
    Template(#[from] aurorix_template::TemplateError),
    #[error("按钮构建失败: {0}")]
    Button(#[from] aurorix_buttons::ButtonError),
    #[error("参数不合法: {0}")]
    Invalid(String),
}

pub type SdkResult<T> = Result<T, SdkError>;
