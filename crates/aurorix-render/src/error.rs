use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum RenderError {
    #[error("渲染调度器已关闭, 不再接收新任务")]
    Shutdown,
    #[error("渲染队列已满, 无法接收新任务")]
    QueueFull,
    #[error("渲染字节预算不足, 无法接收新任务")]
    BytesBudgetExceeded,
    #[error("渲染任务执行超时")]
    Timeout,
    #[error("渲染任务已取消")]
    Cancelled,
    #[error("渲染任务执行过程中发生 panic")]
    Panicked,
    #[error("渲染后端错误: {0}")]
    Backend(String),
    #[error("字体资源错误: {0}")]
    Font(String),
    #[error("纹理资源错误: {0}")]
    Texture(String),
    #[error("io 错误: {0}")]
    Io(String),
    #[error("渲染线程池队列已满, 拒绝新任务 (容量 {capacity})")]
    PoolQueueFull { capacity: usize },
    #[error("编码参数非法: {0}")]
    Encode(String),
}

impl RenderError {
    pub fn enqueue_full(capacity: usize) -> Self {
        Self::PoolQueueFull { capacity }
    }

    pub fn encode(message: impl Into<String>) -> Self {
        Self::Encode(message.into())
    }
}

pub type RenderResult<T> = Result<T, RenderError>;
