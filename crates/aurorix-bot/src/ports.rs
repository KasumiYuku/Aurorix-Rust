use std::collections::BTreeMap;
use std::sync::Arc;

use aurorix_admin::{InboxSendError, InboxSender, SourceFuture};
use aurorix_api::{ApiError, BotApi, MediaResult, MediaUpload, Outbound, SendOutcome};
use aurorix_core::{Scene, Target};
use aurorix_db::{Direction, InboxStore, KvStore, StatsBatch, StatsStore};
use aurorix_gateway::{BoxFuture, GatewayError, TokenSource};
use aurorix_plugin::RenderCard;
use aurorix_render::{
    CanvasKit, Color, RenderContext, RenderError, RenderRequest, RenderScheduler, RenderTask,
    TextStyle,
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde_json::Value;

pub const IMAGE_UPLOAD_UNSUPPORTED: &str = "当前发送通道未实现图片上传, 渲染图发不出去";

pub struct TransportInboxSender {
    transport: Arc<dyn ReplyTransport>,
}

impl TransportInboxSender {
    pub fn new(transport: Arc<dyn ReplyTransport>) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &Arc<dyn ReplyTransport> {
        &self.transport
    }
}

impl std::fmt::Debug for TransportInboxSender {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransportInboxSender")
            .finish_non_exhaustive()
    }
}

impl InboxSender for TransportInboxSender {
    fn send<'a>(
        &'a self,
        target: &'a Target,
        text: &'a str,
    ) -> SourceFuture<'a, Result<(), InboxSendError>> {
        Box::pin(async move {
            let outbound = match target.scene.guild() {
                true => Outbound::text(aurorix_api::guild_text(text)),
                false => Outbound::markdown(text),
            }
            .as_initiative();
            self.transport
                .send(target, &outbound)
                .await
                .map(|_| ())
                .map_err(|error| InboxSendError::Failed(error.to_string()))
        })
    }
}

pub trait ReplyTransport: Send + Sync {
    fn send<'a>(
        &'a self,
        target: &'a Target,
        message: &'a Outbound,
    ) -> BoxFuture<'a, Result<SendOutcome, ApiError>>;

    fn recall<'a>(
        &'a self,
        target: &'a Target,
        message_id: &'a str,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        let _ = (target, message_id);
        Box::pin(async move { Ok(()) })
    }

    fn ack_interaction<'a>(
        &'a self,
        interaction_id: &'a str,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        let _ = interaction_id;
        Box::pin(async move { Ok(()) })
    }

    fn upload_image<'a>(
        &'a self,
        target: &'a Target,
        data: Vec<u8>,
        filename: String,
    ) -> BoxFuture<'a, Result<MediaResult, ApiError>> {
        let _ = (target, data, filename);
        Box::pin(async move { Err(ApiError::Config(String::from(IMAGE_UPLOAD_UNSUPPORTED))) })
    }

    fn send_image_bytes<'a>(
        &'a self,
        target: &'a Target,
        content: String,
        data: Vec<u8>,
        filename: String,
        mime_type: String,
        msg_id: Option<String>,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        let _ = (target, content, data, filename, mime_type, msg_id);
        Box::pin(async move { Err(ApiError::Config(String::from(IMAGE_BYTES_UNSUPPORTED))) })
    }
    fn send_channel_image<'a>(
        &'a self,
        target: &'a Target,
        content: String,
        url: String,
        msg_id: Option<String>,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        let _ = (target, content, url, msg_id);
        Box::pin(async move { Err(ApiError::Config(String::from(IMAGE_URL_UNSUPPORTED))) })
    }
    fn upload_media<'a>(
        &'a self,
        target: &'a Target,
        upload: &'a MediaUpload,
    ) -> BoxFuture<'a, Result<MediaResult, ApiError>> {
        let _ = (target, upload);
        Box::pin(async move { Err(ApiError::Config(String::from(MEDIA_UPLOAD_UNSUPPORTED))) })
    }
    fn conversation_name<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Option<String>> {
        let _ = target;
        Box::pin(async move { None })
    }
}

pub const IMAGE_BYTES_UNSUPPORTED: &str = "该传输不支持频道图片字节直传";


pub const IMAGE_URL_UNSUPPORTED: &str = "该传输不支持频道图片 URL 直发";
pub const MEDIA_UPLOAD_UNSUPPORTED: &str = "该传输不支持媒体上传";

impl ReplyTransport for BotApi {
    fn ack_interaction<'a>(
        &'a self,
        interaction_id: &'a str,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move { BotApi::ack_interaction(self, interaction_id).await })
    }

    fn send<'a>(
        &'a self,
        target: &'a Target,
        message: &'a Outbound,
    ) -> BoxFuture<'a, Result<SendOutcome, ApiError>> {
        Box::pin(async move { BotApi::send(self, target, message).await })
    }

    fn recall<'a>(
        &'a self,
        target: &'a Target,
        message_id: &'a str,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move { BotApi::recall(self, target, message_id, true).await })
    }

    fn upload_image<'a>(
        &'a self,
        target: &'a Target,
        data: Vec<u8>,
        filename: String,
    ) -> BoxFuture<'a, Result<MediaResult, ApiError>> {
        Box::pin(async move { BotApi::upload_image(self, target, data, filename).await })
    }

    fn send_image_bytes<'a>(
        &'a self,
        target: &'a Target,
        content: String,
        data: Vec<u8>,
        filename: String,
        mime_type: String,
        msg_id: Option<String>,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        Box::pin(async move {
            BotApi::send_channel_image_bytes(
                self,
                target,
                &content,
                msg_id.as_deref(),
                &filename,
                &mime_type,
                &data,
            )
            .await
        })
    }

    fn send_channel_image<'a>(
        &'a self,
        target: &'a Target,
        content: String,
        url: String,
        msg_id: Option<String>,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        Box::pin(async move {
            BotApi::send_channel_image_url(self, target, &content, msg_id.as_deref(), &url).await
        })
    }

    fn upload_media<'a>(
        &'a self,
        target: &'a Target,
        upload: &'a MediaUpload,
    ) -> BoxFuture<'a, Result<MediaResult, ApiError>> {
        Box::pin(async move { BotApi::upload_media(self, target, upload).await })
    }

    fn conversation_name<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move {
            if target.scene != Scene::Group {
                return None;
            }
            match BotApi::group_info(self, &target.id).await {
                Ok(info) if !info.name().is_empty() => Some(info.name().to_string()),
                _ => None,
            }
        })
    }
}

pub struct ApiTokens {
    api: Arc<BotApi>,
}

impl ApiTokens {
    pub fn new(api: Arc<BotApi>) -> Self {
        Self { api }
    }
}

impl TokenSource for ApiTokens {
    fn token(&self) -> BoxFuture<'_, Result<String, GatewayError>> {
        Box::pin(async move {
            self.api
                .access_token()
                .await
                .map_err(|err| GatewayError::Token(err.to_string()))
        })
    }

    fn invalidate(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.api.invalidate_token();
        })
    }
}

pub struct RejectedTokens {
    reason: String,
}

impl RejectedTokens {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl Default for RejectedTokens {
    fn default() -> Self {
        Self::new("未注入令牌来源(建议用 with_api 挂上 BotApi, 或自己实现 TokenSource)")
    }
}

impl TokenSource for RejectedTokens {
    fn token(&self) -> BoxFuture<'_, Result<String, GatewayError>> {
        let reason = self.reason.clone();
        Box::pin(async move { Err(GatewayError::Token(reason)) })
    }

    fn invalidate(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {})
    }
}

pub trait KvBackend: Send + Sync {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Value>, String>>;
    fn set<'a>(&'a self, key: &'a str, value: Value) -> BoxFuture<'a, Result<(), String>>;
    fn incr<'a>(&'a self, key: &'a str, delta: i64) -> BoxFuture<'a, Result<i64, String>>;
    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {})
    }
}

impl KvBackend for KvStore {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Value>, String>> {
        Box::pin(async move {
            KvStore::get::<Value>(self, key)
                .await
                .map_err(|err| err.to_string())
        })
    }

    fn set<'a>(&'a self, key: &'a str, value: Value) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            KvStore::set(self, key, &value)
                .await
                .map_err(|err| err.to_string())
        })
    }

    fn incr<'a>(&'a self, key: &'a str, delta: i64) -> BoxFuture<'a, Result<i64, String>> {
        Box::pin(async move {
            KvStore::incr(self, key, delta)
                .await
                .map_err(|err| err.to_string())
        })
    }
}

#[derive(Debug, Default)]
pub struct MemoryKv {
    values: DashMap<String, Value>,
}

impl MemoryKv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl KvBackend for MemoryKv {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Value>, String>> {
        let value = self.values.get(key).map(|entry| entry.clone());
        Box::pin(async move { Ok(value) })
    }

    fn set<'a>(&'a self, key: &'a str, value: Value) -> BoxFuture<'a, Result<(), String>> {
        self.values.insert(key.to_string(), value);
        Box::pin(async move { Ok(()) })
    }

    fn incr<'a>(&'a self, key: &'a str, delta: i64) -> BoxFuture<'a, Result<i64, String>> {
        let mut entry = self.values.entry(key.to_string()).or_insert(Value::from(0));
        let current = entry.as_i64().unwrap_or_default();
        let next = current + delta;
        *entry = Value::from(next);
        Box::pin(async move { Ok(next) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxMessage {
    pub conversation: String,
    pub scene: Scene,
    pub direction: Direction,
    pub sender_id: String,
    pub sender_name: String,
    pub content: String,
    pub platform_id: String,
    pub created_at: DateTime<Utc>,
}

impl InboxMessage {
    pub fn inbound(
        conversation: impl Into<String>,
        scene: Scene,
        sender_id: impl Into<String>,
        sender_name: impl Into<String>,
        content: impl Into<String>,
        platform_id: impl Into<String>,
    ) -> Self {
        Self {
            conversation: conversation.into(),
            scene,
            direction: Direction::Inbound,
            sender_id: sender_id.into(),
            sender_name: sender_name.into(),
            content: content.into(),
            platform_id: platform_id.into(),
            created_at: Utc::now(),
        }
    }

    pub fn outbound(
        conversation: impl Into<String>,
        scene: Scene,
        content: impl Into<String>,
        platform_id: impl Into<String>,
    ) -> Self {
        Self {
            conversation: conversation.into(),
            scene,
            direction: Direction::Outbound,
            sender_id: String::new(),
            sender_name: String::new(),
            content: content.into(),
            platform_id: platform_id.into(),
            created_at: Utc::now(),
        }
    }

    fn to_db_message(&self) -> aurorix_db::Message {
        aurorix_db::Message {
            id: 0,
            conv_id: self.conversation.clone(),
            kind: self.scene,
            direction: self.direction,
            sender_id: self.sender_id.clone(),
            sender_name: self.sender_name.clone(),
            content: self.content.clone(),
            platform_id: self.platform_id.clone(),
            created_at: self.created_at,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxBatch {
    pub messages: Vec<InboxMessage>,
}

impl InboxBatch {
    pub fn new(messages: Vec<InboxMessage>) -> Self {
        Self { messages }
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn push(&mut self, message: InboxMessage) {
        self.messages.push(message);
    }
}

pub trait InboxArchive: Send + Sync {
    fn save(&self, batch: InboxBatch) -> BoxFuture<'_, Result<(), String>>;
    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {})
    }

    fn name_of(&self, conv_id: String) -> BoxFuture<'_, Result<String, String>> {
        let _ = conv_id;
        Box::pin(async move { Ok(String::new()) })
    }

    fn set_name(&self, conv_id: String, name: String) -> BoxFuture<'_, Result<(), String>> {
        let _ = (conv_id, name);
        Box::pin(async move { Ok(()) })
    }
}

impl InboxArchive for InboxStore {
    fn save(&self, batch: InboxBatch) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            for message in &batch.messages {
                InboxStore::save(self, &message.to_db_message())
                    .await
                    .map_err(|err| err.to_string())?;
            }
            Ok(())
        })
    }

    fn name_of(&self, conv_id: String) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(async move {
            InboxStore::name_of(self, &conv_id)
                .await
                .map_err(|err| err.to_string())
        })
    }

    fn set_name(&self, conv_id: String, name: String) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            InboxStore::set_name(self, &conv_id, &name)
                .await
                .map_err(|err| err.to_string())
        })
    }
}

#[derive(Debug, Default)]
pub struct MemoryInbox {
    batches: DashMap<u64, InboxBatch>,
    names: DashMap<String, String>,
    next_id: std::sync::atomic::AtomicU64,
}

impl MemoryInbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn saved(&self) -> Vec<InboxBatch> {
        let mut items: Vec<(u64, InboxBatch)> = self
            .batches
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();
        items.sort_by_key(|(id, _)| *id);
        items.into_iter().map(|(_, batch)| batch).collect()
    }

    pub fn messages(&self) -> Vec<InboxMessage> {
        self.saved()
            .into_iter()
            .flat_map(|batch| batch.messages)
            .collect()
    }

    pub fn names(&self) -> std::collections::BTreeMap<String, String> {
        self.names
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }
}

impl InboxArchive for MemoryInbox {
    fn save(&self, batch: InboxBatch) -> BoxFuture<'_, Result<(), String>> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.batches.insert(id, batch);
        Box::pin(async move { Ok(()) })
    }

    fn name_of(&self, conv_id: String) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(async move {
            Ok(self
                .names
                .get(&conv_id)
                .map(|v| v.clone())
                .unwrap_or_default())
        })
    }

    fn set_name(&self, conv_id: String, name: String) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.names.insert(conv_id, name);
            Ok(())
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatsDelta {
    pub scene: Scene,
    pub conversation: String,
    pub name: String,
    pub received: u64,
    pub sent: u64,
    pub button: u64,
    pub day: String,
}

pub trait StatsRecorder: Send + Sync {
    fn record(&self, delta: StatsDelta) -> BoxFuture<'_, Result<(), String>>;

    fn set_name(
        &self,
        scene: Scene,
        conversation: String,
        name: String,
    ) -> BoxFuture<'_, Result<(), String>> {
        let _ = (scene, conversation, name);
        Box::pin(async move { Ok(()) })
    }
    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {})
    }
}

#[derive(Clone)]
pub struct DbStats {
    store: Arc<StatsStore>,
}

impl DbStats {
    pub fn new(store: Arc<StatsStore>) -> Self {
        Self { store }
    }
}

impl StatsRecorder for DbStats {
    fn set_name(
        &self,
        scene: Scene,
        conversation: String,
        name: String,
    ) -> BoxFuture<'_, Result<(), String>> {
        let store = Arc::clone(&self.store);
        Box::pin(async move {
            store
                .set_name(scene, &conversation, &name)
                .await
                .map_err(|err| err.to_string())
        })
    }

    fn record(&self, delta: StatsDelta) -> BoxFuture<'_, Result<(), String>> {
        let store = Arc::clone(&self.store);
        Box::pin(async move {
            let mut batch = StatsBatch::default();
            for _ in 0..delta.received {
                batch.received(delta.scene, &delta.conversation, &delta.name);
            }
            if delta.received > 0 || delta.sent > 0 || delta.button > 0 {
                batch.count_day(
                    &delta.day,
                    delta.received as i64,
                    delta.sent as i64,
                    delta.button as i64,
                );
            }
            if batch.is_empty() {
                return Ok(());
            }
            store
                .write_batch(&batch)
                .await
                .map_err(|err| err.to_string())
        })
    }
}

#[derive(Debug, Default)]
pub struct MemoryStats {
    deltas: DashMap<u64, StatsDelta>,
    names: DashMap<String, String>,
    next_id: std::sync::atomic::AtomicU64,
}

impl MemoryStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn calls(&self) -> usize {
        self.deltas.len()
    }

    pub fn deltas(&self) -> Vec<StatsDelta> {
        let mut items: Vec<(u64, StatsDelta)> = self
            .deltas
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();
        items.sort_by_key(|(id, _)| *id);
        items.into_iter().map(|(_, delta)| delta).collect()
    }

    pub fn total_received(&self) -> u64 {
        self.deltas.iter().map(|entry| entry.value().received).sum()
    }

    pub fn names(&self) -> std::collections::BTreeMap<String, String> {
        self.names
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    pub fn total_sent(&self) -> u64 {
        self.deltas.iter().map(|entry| entry.value().sent).sum()
    }
}

impl StatsRecorder for MemoryStats {
    fn set_name(
        &self,
        _scene: Scene,
        conversation: String,
        name: String,
    ) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.names.insert(conversation, name);
            Ok(())
        })
    }

    fn record(&self, delta: StatsDelta) -> BoxFuture<'_, Result<(), String>> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.deltas.insert(id, delta);
        Box::pin(async move { Ok(()) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardView {
    pub scene: Scene,
    pub title: String,
    pub body: String,
}

impl CardView {
    pub fn new(scene: Scene, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            scene,
            title: title.into(),
            body: body.into(),
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.title.trim().is_empty() {
            lines.push(self.title.clone());
        }
        for line in self.body.lines() {
            if !line.trim().is_empty() {
                lines.push(line.to_string());
            }
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rendered {
    Text(String),
    Image { bytes: Vec<u8>, filename: String },
}

pub const CUSTOM_RENDER_UNSUPPORTED: &str = "该渲染器不支持插件自定义绘图";

pub trait Renderer: Send + Sync {
    fn render<'a>(&'a self, card: &'a CardView) -> BoxFuture<'a, Result<Rendered, RenderError>>;

    fn render_card<'a>(
        &'a self,
        card: &'a RenderCard,
    ) -> BoxFuture<'a, Result<Rendered, RenderError>> {
        let _ = card;
        Box::pin(async move {
            Err(RenderError::Backend(String::from(
                CUSTOM_RENDER_UNSUPPORTED,
            )))
        })
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {})
    }
}

pub const CARD_WIDTH: i32 = 720;

pub const CARD_FONT_SIZE: u32 = 24;

pub const CARD_FILENAME: &str = "card.png";

pub struct SchedulerRenderer {
    scheduler: Arc<RenderScheduler>,
    contexts: BTreeMap<String, Arc<RenderContext>>,
    width: i32,
    font_size: u32,
    fallback: Option<Arc<RenderContext>>,
}

impl SchedulerRenderer {
    pub fn new(scheduler: Arc<RenderScheduler>) -> Self {
        Self {
            scheduler,
            contexts: BTreeMap::new(),
            fallback: None,
            width: CARD_WIDTH,
            font_size: CARD_FONT_SIZE,
        }
    }

    pub fn with_contexts(mut self, contexts: BTreeMap<String, Arc<RenderContext>>) -> Self {
        self.contexts = contexts;
        self
    }

    pub fn with_fallback_context(mut self, context: Arc<RenderContext>) -> Self {
        self.fallback = Some(context);
        self
    }

    pub fn with_size(mut self, width: i32, font_size: u32) -> Self {
        self.width = width.max(64);
        self.font_size = font_size.max(8);
        self
    }

    pub fn scheduler(&self) -> &Arc<RenderScheduler> {
        &self.scheduler
    }
}

impl Renderer for SchedulerRenderer {
    fn render<'a>(&'a self, card: &'a CardView) -> BoxFuture<'a, Result<Rendered, RenderError>> {
        let lines = card.lines();
        let width = self.width;
        let font_size = self.font_size;
        let line_height = (font_size as f32) * 1.4;
        let height = (((lines.len() as f32) * line_height) + (line_height * 2.0)).max(96.0) as i32;
        Box::pin(async move {
            let request = RenderRequest::new(width, height).with_background(Color::WHITE);
            let task = RenderTask::new("card").with_estimated_bytes(request.estimated_bytes());
            let handle = self
                .scheduler
                .submit_render(task, request, move |canvas, context| {
                    let style = TextStyle::new("sans-serif", font_size);
                    let mut y = line_height * 1.5;
                    for line in &lines {
                        let _ = context.draw_text(canvas, line, 16.0, y, &style);
                        y += line_height;
                    }
                })
                .await?;
            let output = handle.wait().await?;
            Ok(Rendered::Image {
                bytes: output.bytes.to_vec(),
                filename: String::from(CARD_FILENAME),
            })
        })
    }

    fn render_card<'a>(
        &'a self,
        card: &'a RenderCard,
    ) -> BoxFuture<'a, Result<Rendered, RenderError>> {
        Box::pin(async move {
            let request = card.request;
            let painter = Arc::clone(&card.painter);
            let label = card.label.clone();
            let key = card.cache_key;
            let scheduler = Arc::clone(&self.scheduler);
            let context = card
                .plugin
                .as_ref()
                .and_then(|id| self.contexts.get(id).cloned())
                .or_else(|| self.fallback.clone());
            let work = async move {
                let task = RenderTask::new("plugin-card")
                    .with_estimated_bytes(request.estimated_bytes());
                let handle = scheduler
                    .submit_render(task, request, move |canvas, fallback| {
                        match context.as_ref() {
                            Some(context) => painter(&CanvasKit::new(canvas, context)),
                            None => painter(&CanvasKit::new(canvas, fallback)),
                        }
                    })
                    .await?;
                let output = handle.wait().await?;
                Ok(output.bytes.to_vec())
            };
            match key {
                Some(key) => crate::card_cache::cached(key, label, work).await,
                None => Ok(Rendered::Image {
                    bytes: work.await?,
                    filename: label,
                }),
            }
        })
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        let scheduler = Arc::clone(&self.scheduler);
        Box::pin(async move {
            scheduler.shutdown().await;
        })
    }
}

pub trait PluginConfigurable: Send + Sync {
    fn plugin_id(&self) -> &str;

    fn apply(&self, settings: &BTreeMap<String, Value>) -> Result<(), String>;
}

pub struct ClosurePluginConfig<F> {
    id: String,
    apply: F,
}

impl<F> ClosurePluginConfig<F>
where
    F: Fn(&BTreeMap<String, Value>) -> Result<(), String> + Send + Sync,
{
    pub fn new(id: impl Into<String>, apply: F) -> Self {
        Self {
            id: id.into(),
            apply,
        }
    }
}

impl<F> PluginConfigurable for ClosurePluginConfig<F>
where
    F: Fn(&BTreeMap<String, Value>) -> Result<(), String> + Send + Sync,
{
    fn plugin_id(&self) -> &str {
        &self.id
    }

    fn apply(&self, settings: &BTreeMap<String, Value>) -> Result<(), String> {
        (self.apply)(settings)
    }
}
