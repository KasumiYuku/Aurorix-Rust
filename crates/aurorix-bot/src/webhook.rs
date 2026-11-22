use std::sync::Arc;

use aurorix_gateway::{
    ensure_webhook_compatible, webhook_router, EventSink, WebhookProcessor, WEBHOOK_PATH,
};
use axum::Router;

use crate::error::BotError;

pub const MODULE: &str = "webhook";

pub const WEBHOOK_MODE_LABEL: &str = "Webhook 接收模式已启用";

pub struct WebhookIngress {
    router: Router,
}

impl WebhookIngress {
    pub fn new(app_secret: impl Into<String>, sink: Arc<dyn EventSink>) -> Self {
        let processor = Arc::new(WebhookProcessor::new(app_secret, sink));
        Self {
            router: webhook_router(processor),
        }
    }

    pub fn path() -> &'static str {
        WEBHOOK_PATH
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }

    pub fn into_router(self) -> Router {
        self.router
    }
}

pub fn webhook_events(app_secret: impl Into<String>, sink: Arc<dyn EventSink>) -> WebhookIngress {
    WebhookIngress::new(app_secret, sink)
}

pub fn ensure_webhook_intents(intents: &[String]) -> Result<(), BotError> {
    ensure_webhook_compatible(intents).map_err(|err| {
        BotError::Config(format!(
            "webhook 模式接不住当前 intents: {err}; 频道事件只能通过 WebSocket 接收, 请把 intents 里的频道事件去掉, 或把 protocol 改成 websocket"
        ))
    })
}
