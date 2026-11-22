use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use aurorix_core::{RoleRequired, Scene, Target};
use aurorix_db::PluginDb;

use crate::args::ParsedArgs;
use crate::assets::PluginAssets;
use crate::error::PluginError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyKind {
    Maintenance,
    Denied,
    Banned,
    RateLimited,
    ArgumentError,
    Handler,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub data: Vec<u8>,
    pub filename: String,
    pub mime_type: String,
}

impl Attachment {
    pub fn new(
        data: impl Into<Vec<u8>>,
        filename: impl Into<String>,
        mime_type: impl Into<String>,
    ) -> Self {
        Self {
            data: data.into(),
            filename: filename.into(),
            mime_type: mime_type.into(),
        }
    }

    pub fn png(data: impl Into<Vec<u8>>) -> Self {
        Self::new(data, "card.png", "image/png")
    }

    pub fn jpeg(data: impl Into<Vec<u8>>) -> Self {
        Self::new(data, "card.jpg", "image/jpeg")
    }

    pub fn webp(data: impl Into<Vec<u8>>) -> Self {
        Self::new(data, "card.webp", "image/webp")
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

pub const MEDIA_IMAGE: i32 = 1;
pub const MEDIA_VIDEO: i32 = 2;
pub const MEDIA_VOICE: i32 = 3;
pub const MEDIA_FILE: i32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Media {
    Source(String),
    Bytes {
        data: Vec<u8>,
        filename: String,
    },
}

impl Media {
    pub fn source(text: impl Into<String>) -> Self {
        Self::Source(text.into())
    }

    pub fn bytes(data: impl Into<Vec<u8>>, filename: impl Into<String>) -> Self {
        Self::Bytes {
            data: data.into(),
            filename: filename.into(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Source(text) => text.trim().is_empty(),
            Self::Bytes { data, .. } => data.is_empty(),
        }
    }
}

impl From<&str> for Media {
    fn from(value: &str) -> Self {
        Self::source(value)
    }
}

impl From<String> for Media {
    fn from(value: String) -> Self {
        Self::source(value)
    }
}

impl From<&String> for Media {
    fn from(value: &String) -> Self {
        Self::source(value.clone())
    }
}

impl From<Vec<u8>> for Media {
    fn from(value: Vec<u8>) -> Self {
        Self::bytes(value, String::new())
    }
}

impl From<&[u8]> for Media {
    fn from(value: &[u8]) -> Self {
        Self::bytes(value.to_vec(), String::new())
    }
}

pub type Painter = Arc<dyn Fn(&aurorix_render::CanvasKit<'_>) + Send + Sync>;

#[derive(Clone)]
pub struct RenderCard {
    pub request: aurorix_render::RenderRequest,
    pub painter: Painter,
    pub label: String,
    pub cache_key: Option<u64>,
    pub plugin: Option<String>,
}

impl std::fmt::Debug for RenderCard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RenderCard")
            .field("request", &self.request)
            .field("label", &self.label)
            .field("cache_key", &self.cache_key)
            .finish()
    }
}

impl RenderCard {
    pub fn new<F>(
        request: aurorix_render::RenderRequest,
        label: impl Into<String>,
        painter: F,
    ) -> Self
    where
        F: Fn(&aurorix_render::CanvasKit<'_>) + Send + Sync + 'static,
    {
        Self {
            request,
            painter: Arc::new(painter),
            label: label.into(),
            cache_key: None,
            plugin: None,
        }
    }

    pub fn plugin(mut self, plugin: impl Into<String>) -> Self {
        self.plugin = Some(plugin.into());
        self
    }

    pub fn with_cache_key(mut self, key: u64) -> Self {
        self.cache_key = Some(key);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncomingAttachment {
    pub url: String,
    pub content_type: String,
    pub filename: String,
    pub width: u32,
    pub height: u32,
}

impl IncomingAttachment {
    pub fn new(url: impl Into<String>, content_type: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            content_type: content_type.into(),
            filename: String::new(),
            width: 0,
            height: 0,
        }
    }

    pub fn image(&self) -> bool {
        self.content_type.starts_with("image/")
            || (self.content_type.is_empty() && looks_image(&self.url))
    }

    pub fn is_empty(&self) -> bool {
        self.url.is_empty()
    }
}

fn looks_image(url: &str) -> bool {
    let lowered = url.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"]
        .iter()
        .any(|suffix| lowered.contains(suffix))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource {
    Bytes(Attachment),
    Url { url: String, summary: String },
}

impl ImageSource {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Bytes(attachment) => attachment.is_empty(),
            Self::Url { url, .. } => url.trim().is_empty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtPart {
    Mention { openid: String, newline: bool },
    All { newline: bool },
    Avatar { openid: String, size: u32 },
}

impl AtPart {
    pub fn newline(&self) -> bool {
        match self {
            Self::Mention { newline, .. } | Self::All { newline } => *newline,
            Self::Avatar { .. } => false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Part {
    Text(String),
    Markdown(String),
    At(AtPart),
    Image(ImageSource),
    Media {
        kind: i32,
        media: Media,
    },
    Card {
        card: RenderCard,
        fallback: Option<String>,
    },
}

impl PartialEq for Part {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(left), Self::Text(right)) => left == right,
            (Self::Markdown(left), Self::Markdown(right)) => left == right,
            (Self::At(left), Self::At(right)) => left == right,
            (Self::Image(left), Self::Image(right)) => left == right,
            (
                Self::Media {
                    kind: left_kind,
                    media: left_media,
                },
                Self::Media {
                    kind: right_kind,
                    media: right_media,
                },
            ) => left_kind == right_kind && left_media == right_media,
            (Self::Card { .. }, Self::Card { .. }) => true,
            _ => false,
        }
    }
}

impl Eq for Part {}

#[derive(Debug, Clone)]
pub struct Outgoing {
    pub kind: ReplyKind,
    pub parts: Vec<Part>,
    pub keyboard: Option<aurorix_buttons::Keyboard>,
    pub quote: Option<String>,
    pub mention_user: Option<String>,
    pub at: bool,
    pub no_footer: bool,
    pub image_only: bool,
    pub guide: bool,
}

impl PartialEq for Outgoing {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.parts == other.parts
            && self.keyboard == other.keyboard
            && self.quote == other.quote
            && self.mention_user == other.mention_user
            && self.at == other.at
            && self.no_footer == other.no_footer
            && self.image_only == other.image_only
    }
}

impl Eq for Outgoing {}

impl Outgoing {
    pub fn new(kind: ReplyKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            parts: vec![Part::Text(text.into())],
            keyboard: None,
            quote: None,
            mention_user: None,
            at: false,
            no_footer: false,
            image_only: false,
            guide: false,
        }
    }

    pub fn with_guide(mut self) -> Self {
        self.guide = true;
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.parts.push(Part::Text(text.into()));
        self
    }

    pub fn markdown(mut self, markdown: impl Into<String>) -> Self {
        self.parts.push(Part::Markdown(markdown.into()));
        self
    }

    pub fn at(mut self, openid: impl Into<String>) -> Self {
        self.parts.push(Part::At(AtPart::Mention {
            openid: openid.into(),
            newline: false,
        }));
        self
    }

    pub fn at_all(mut self) -> Self {
        self.parts.push(Part::At(AtPart::All { newline: false }));
        self
    }

    pub fn at_avatar(mut self, openid: impl Into<String>, size: u32) -> Self {
        self.parts.push(Part::At(AtPart::Avatar {
            openid: openid.into(),
            size,
        }));
        self
    }

    pub fn image(mut self, attachment: Attachment) -> Self {
        self.parts.push(Part::Image(ImageSource::Bytes(attachment)));
        self
    }

    pub fn image_url(mut self, url: impl Into<String>, summary: impl Into<String>) -> Self {
        self.parts.push(Part::Image(ImageSource::Url {
            url: url.into(),
            summary: summary.into(),
        }));
        self
    }

    pub fn voice(mut self, source: impl Into<Media>) -> Self {
        self.parts.push(Part::Media {
            kind: MEDIA_VOICE,
            media: source.into(),
        });
        self
    }

    pub fn video(mut self, source: impl Into<Media>) -> Self {
        self.parts.push(Part::Media {
            kind: MEDIA_VIDEO,
            media: source.into(),
        });
        self
    }

    pub fn file(mut self, source: impl Into<Media>) -> Self {
        self.parts.push(Part::Media {
            kind: MEDIA_FILE,
            media: source.into(),
        });
        self
    }

    pub fn card(mut self, card: RenderCard) -> Self {
        self.parts.push(Part::Card {
            card,
            fallback: None,
        });
        self
    }

    pub fn card_or(mut self, card: RenderCard, fallback: impl Into<String>) -> Self {
        self.parts.push(Part::Card {
            card,
            fallback: Some(fallback.into()),
        });
        self
    }

    pub fn keyboard(mut self, keyboard: aurorix_buttons::Keyboard) -> Self {
        self.keyboard = Some(keyboard);
        self
    }

    pub fn reply_to(mut self, message_id: impl Into<String>) -> Self {
        self.quote = Some(message_id.into());
        self
    }

    pub fn mention_user(mut self, user_id: impl Into<String>) -> Self {
        self.mention_user = Some(user_id.into());
        self
    }

    pub fn no_footer(mut self) -> Self {
        self.no_footer = true;
        self
    }

    pub fn stamp_plugin(&mut self, plugin: &str) {
        for part in &mut self.parts {
            if let Part::Card { card, .. } = part {
                if card.plugin.is_none() {
                    card.plugin = Some(plugin.to_string());
                }
            }
        }
    }

    pub fn image_only(mut self) -> Self {
        self.image_only = true;
        self
    }

    pub fn body(&self) -> String {
        let mut body = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) | Part::Markdown(text) => body.push_str(text),
                _ => {}
            }
        }
        body
    }

    pub fn is_empty(&self) -> bool {
        if self.keyboard.is_some() {
            return false;
        }
        self.parts.iter().all(|part| match part {
            Part::Text(text) | Part::Markdown(text) => text.trim().is_empty(),
            Part::At(_) => false,
            Part::Image(source) => source.is_empty(),
            Part::Media { media, .. } => media.is_empty(),
            Part::Card { .. } => false,
        })
    }

    pub fn has_render(&self) -> bool {
        self.parts
            .iter()
            .any(|part| matches!(part, Part::Card { .. }))
    }

    pub fn has_image(&self) -> bool {
        self.parts
            .iter()
            .any(|part| matches!(part, Part::Image(source) if !source.is_empty()))
    }
}

pub type SendFuture = Pin<Box<dyn Future<Output = Result<(), PluginError>> + Send>>;

type SendFn = dyn Fn(Outgoing) -> SendFuture + Send + Sync;

#[derive(Clone)]
pub struct InvocationSender(Arc<SendFn>);

impl InvocationSender {
    pub fn new<F>(send: F) -> Self
    where
        F: Fn(Outgoing) -> SendFuture + Send + Sync + 'static,
    {
        Self(Arc::new(send))
    }

    pub async fn send(&self, outgoing: Outgoing) -> Result<(), PluginError> {
        (self.0)(outgoing).await
    }
}

impl std::fmt::Debug for InvocationSender {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InvocationSender")
            .finish_non_exhaustive()
    }
}

impl PartialEq for InvocationSender {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for InvocationSender {}

type PushFn = dyn Fn(Target, Outgoing) -> SendFuture + Send + Sync;

#[derive(Clone)]
pub struct PushSink(Arc<PushFn>);

impl PushSink {
    pub fn new<F>(push: F) -> Self
    where
        F: Fn(Target, Outgoing) -> SendFuture + Send + Sync + 'static,
    {
        Self(Arc::new(push))
    }

    pub async fn push(&self, target: Target, outgoing: Outgoing) -> Result<(), PluginError> {
        (self.0)(target, outgoing).await
    }
}

impl std::fmt::Debug for PushSink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PushSink")
            .finish_non_exhaustive()
    }
}

impl PartialEq for PushSink {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for PushSink {}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Invocation {
    pub scene: Scene,
    pub target: Target,
    pub user: String,
    pub username: Option<String>,
    pub plugin: String,
    pub command: String,
    pub args: ParsedArgs,
    pub text: String,
    pub raw: String,
    pub role: Option<RoleRequired>,
    pub no_footer: bool,
    pub opt_plugin_db: Option<Arc<PluginDb>>,
    pub opt_plugin_assets: Option<Arc<PluginAssets>>,
    pub opt_sender: Option<InvocationSender>,
    pub opt_push: Option<PushSink>,
    pub attachments: Vec<IncomingAttachment>,
    pub reference: Option<String>,
}

impl Invocation {
    pub fn with_plugin_db(mut self, plugin_db: Arc<PluginDb>) -> Self {
        self.opt_plugin_db = Some(plugin_db);
        self
    }

    pub fn db(&self) -> Option<&Arc<PluginDb>> {
        self.opt_plugin_db.as_ref()
    }

    pub fn plugin_db(&self) -> Option<Arc<PluginDb>> {
        self.opt_plugin_db.clone()
    }

    pub fn has_plugin_db(&self) -> bool {
        self.opt_plugin_db.is_some()
    }

    pub fn kv(&self) -> Option<&aurorix_db::KvStore> {
        self.opt_plugin_db.as_ref().map(|plugin_db| plugin_db.kv())
    }

    pub fn with_plugin_assets(mut self, assets: Arc<PluginAssets>) -> Self {
        self.opt_plugin_assets = Some(assets);
        self
    }

    pub fn assets(&self) -> Option<&Arc<PluginAssets>> {
        self.opt_plugin_assets.as_ref()
    }

    pub fn opt_plugin_assets(&self) -> Option<Arc<PluginAssets>> {
        self.opt_plugin_assets.clone()
    }

    pub fn has_plugin_assets(&self) -> bool {
        self.opt_plugin_assets.is_some()
    }
}

impl Invocation {
    pub fn from_event(event: &Incoming) -> Self {
        Self {
            scene: event.scene,
            target: event.target.clone(),
            user: event.user.clone(),
            username: event.username.clone(),
            plugin: String::new(),
            command: String::new(),
            args: ParsedArgs::default(),
            text: String::new(),
            raw: event.content.clone(),
            role: None,
            no_footer: false,
            opt_plugin_db: None,
            opt_plugin_assets: None,
            opt_sender: None,
            opt_push: None,
            attachments: event.attachments.clone(),
            reference: event.reference.clone(),
        }
    }

    pub fn with_sender(mut self, sender: InvocationSender) -> Self {
        self.opt_sender = Some(sender);
        self
    }

    pub fn with_push(mut self, sink: PushSink) -> Self {
        self.opt_push = Some(sink);
        self
    }

    pub fn push_sink(&self) -> Option<&PushSink> {
        self.opt_push.as_ref()
    }

    pub fn has_push(&self) -> bool {
        self.opt_push.is_some()
    }

    pub async fn push_to(&self, target: Target, outgoing: Outgoing) -> Result<(), PluginError> {
        match &self.opt_push {
            Some(sink) => sink.push(target, outgoing).await,
            None => Err(PluginError::reply(String::from("当前通道不支持主动推送"))),
        }
    }

    pub fn has_sender(&self) -> bool {
        self.opt_sender.is_some()
    }

    pub async fn send(&self, outgoing: Outgoing) -> Result<(), PluginError> {
        let Some(sender) = self.opt_sender.as_ref() else {
            return Err(PluginError::reply("当前指令没有绑定中途发送通道"));
        };
        sender.send(outgoing).await
    }

    pub fn attachments(&self) -> &[IncomingAttachment] {
        &self.attachments
    }

    pub fn images(&self) -> Vec<&IncomingAttachment> {
        self.attachments
            .iter()
            .filter(|attachment| attachment.image() && !attachment.is_empty())
            .collect()
    }

    pub fn quoted(&self) -> Option<&str> {
        self.reference.as_deref()
    }

    pub fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }

    pub fn mention_target(&self) -> Option<&str> {
        if self.target.id.is_empty() || self.target.user.is_empty() {
            return None;
        }
        Some(self.target.mention())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Incoming {
    pub scene: Scene,
    pub target: Target,
    pub user: String,
    pub content: String,
    pub username: Option<String>,
    pub platform_role: Option<RoleRequired>,
    pub private: bool,
    pub attachments: Vec<IncomingAttachment>,
    pub reference: Option<String>,
}

impl Incoming {
    pub fn new(
        scene: Scene,
        target: Target,
        user: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            scene,
            target,
            user: user.into(),
            content: content.into(),
            username: None,
            platform_role: None,
            private: matches!(scene, Scene::C2c | Scene::Dm),
            attachments: Vec::new(),
            reference: None,
        }
    }

    pub fn with_attachments(mut self, attachments: Vec<IncomingAttachment>) -> Self {
        self.attachments = attachments;
        self
    }

    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        let reference = reference.into();
        self.reference = (!reference.is_empty()).then_some(reference);
        self
    }

    pub fn images(&self) -> Vec<&IncomingAttachment> {
        self.attachments
            .iter()
            .filter(|attachment| attachment.image() && !attachment.is_empty())
            .collect()
    }

    pub fn from_context(context: &aurorix_sdk::Context, content: impl Into<String>) -> Self {
        let user = context
            .user
            .clone()
            .unwrap_or_else(|| context.target.user.clone());
        let mut event = Self::new(context.scene, context.target.clone(), user, content);
        event.username = context.user.clone();
        event
    }

    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    pub fn with_platform_role(mut self, role: RoleRequired) -> Self {
        self.platform_role = Some(role);
        self
    }

    pub fn as_private(mut self, private: bool) -> Self {
        self.private = private;
        self
    }
}

pub trait ReplySink: Send + Sync {
    fn reply(
        &self,
        ctx: &Invocation,
        message: Outgoing,
    ) -> impl Future<Output = Result<(), PluginError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Incoming {
        Incoming::new(
            Scene::Group,
            Target::new(Scene::Group, "g1").with_user("u1"),
            "u1",
            "/echo hi",
        )
    }

    #[test]
    fn outgoing_carries_flags() {
        let mut message = Outgoing::new(ReplyKind::Maintenance, "维护中").no_footer();
        message.at = true;
        message.mention_user = Some(String::from("u9"));
        assert_eq!(message.kind, ReplyKind::Maintenance);
        assert_eq!(message.body(), "维护中");
        assert!(message.at);
        assert!(message.no_footer);
        assert!(!message.is_empty());
        assert!(Outgoing::new(ReplyKind::Denied, "   ").is_empty());
        assert!(Outgoing::new(ReplyKind::Denied, String::new())
            .image(Attachment::png(Vec::new()))
            .is_empty());
        assert!(Outgoing::new(ReplyKind::Denied, String::new())
            .image(Attachment::png(vec![1, 2, 3]))
            .has_image());
        assert!(!Outgoing::new(ReplyKind::Denied, String::new()).has_image());
    }

    #[test]
    fn parts_keep_their_order() {
        let message = Outgoing::new(ReplyKind::Handler, "甲")
            .image_url("https://e.test/a.png", "图一")
            .markdown("乙")
            .at("u2")
            .voice(Media::source("https://e.test/a.silk"));
        assert_eq!(message.body(), "甲乙");
        assert_eq!(message.parts.len(), 5);
        assert!(matches!(message.parts[1], Part::Image(ImageSource::Url { .. })));
        assert!(matches!(
            message.parts[4],
            Part::Media {
                kind: MEDIA_VOICE,
                ..
            }
        ));
        assert_eq!(message, message.clone());
    }

    #[test]
    fn incoming_defaults_private_by_scene() {
        assert!(!event().private, "群聊不是私聊");
        let c2c = Incoming::new(Scene::C2c, Target::new(Scene::C2c, "u1"), "u1", "hi");
        assert!(c2c.private, "私聊默认按私聊处理");
        let dm = Incoming::new(Scene::Dm, Target::new(Scene::Dm, "d1"), "u1", "hi");
        assert!(dm.private);
    }

    #[test]
    fn invocation_mention_requires_both_ids() {
        let invocation = Invocation::from_event(&event());
        assert_eq!(invocation.mention_target(), Some("u1"));
        assert_eq!(invocation.raw, "/echo hi");
        let mut no_user = invocation.clone();
        no_user.target.user.clear();
        assert!(no_user.mention_target().is_none());
        let mut no_target = invocation;
        no_target.target.id.clear();
        assert!(no_target.mention_target().is_none());
    }

    #[test]
    fn incoming_bridges_sdk_context() {
        let context = aurorix_sdk::Context::new(
            Scene::Channel,
            Target::new(Scene::Channel, "c1").with_user("u9"),
        )
        .with_user("u9");
        let event = Incoming::from_context(&context, "/help");
        assert_eq!(event.scene, Scene::Channel);
        assert_eq!(event.user, "u9");
        assert_eq!(event.content, "/help");
        assert!(!event.private, "频道不是私聊");
        assert_eq!(event.username.as_deref(), Some("u9"));
    }

    #[tokio::test]
    async fn invocation_carries_an_optional_plugin_db() {
        let event = event();
        let plain = Invocation::from_event(&event);
        assert!(plain.db().is_none(), "默认不该注入数据库句柄");
        assert!(!plain.has_plugin_db());
        assert!(plain.kv().is_none());
        assert_eq!(plain, Invocation::from_event(&event));

        let pool = sqlx::PgPool::connect_lazy("postgres://aurorix:aurorix@127.0.0.1:55433/aurorix")
            .expect("惰性连接串");
        let registry = aurorix_db::PluginDbRegistry::new(pool, Default::default());
        let plugin_db = registry.plugin("rating").expect("句柄");
        let wired = Invocation::from_event(&event).with_plugin_db(Arc::clone(&plugin_db));
        assert!(wired.has_plugin_db());
        assert_eq!(wired.db().map(|db| db.plugin_id()), Some("rating"));
        assert_eq!(
            wired.plugin_db().map(|db| db.plugin_id().to_string()),
            Some("rating".to_string())
        );
        let kv = wired.kv().expect("插件作用域 kv");
        assert_eq!(kv.scope(), aurorix_db::SCOPE_PLUGIN);
        assert_eq!(kv.namespace(), "rating");
        assert_ne!(kv.namespace(), aurorix_db::SCOPE_GLOBAL);

        let other =
            Invocation::from_event(&event).with_plugin_db(registry.plugin("arcaea").expect("句柄"));
        assert_ne!(wired.db(), other.db(), "不同插件的句柄不该相等");
        assert_eq!(wired.clone(), wired, "克隆后仍然相等");
        assert_ne!(wired, other);
    }
}

#[cfg(test)]
mod sender_tests {
    use super::*;
    use std::sync::Mutex;

    fn event() -> Incoming {
        Incoming::new(
            Scene::Group,
            Target::new(Scene::Group, "g1").with_user("u1"),
            "u1",
            "/query",
        )
    }

    #[tokio::test]
    async fn invocation_without_sender_reports_a_clear_error() {
        let invocation = Invocation::from_event(&event());
        assert!(!invocation.has_sender(), "默认构造不该带发送通道");
        let error = invocation
            .send(Outgoing::new(ReplyKind::Handler, "正在查询…"))
            .await
            .expect_err("没有通道时必须报错");
        assert!(
            error.to_string().contains("中途发送通道"),
            "错误信息要说清楚原因, 实际是 {error}"
        );
    }

    #[tokio::test]
    async fn sender_keeps_every_message_in_order() {
        let sent: Arc<Mutex<Vec<Outgoing>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&sent);
        let sender = InvocationSender::new(move |outgoing: Outgoing| {
            let recorder = Arc::clone(&recorder);
            Box::pin(async move {
                recorder.lock().expect("记录锁可用").push(outgoing);
                Ok(())
            })
        });
        let invocation = Invocation::from_event(&event()).with_sender(sender.clone());
        assert!(invocation.has_sender(), "绑定后必须报告已具备通道");
        invocation
            .send(Outgoing::new(ReplyKind::Handler, "正在查询…"))
            .await
            .expect("第一条应当发出");
        invocation
            .send(Outgoing::new(ReplyKind::Handler, "还在查询…"))
            .await
            .expect("第二条应当发出");
        let texts: Vec<String> = sent
            .lock()
            .expect("记录锁可用")
            .iter()
            .map(|item| item.body())
            .collect();
        assert_eq!(texts, vec!["正在查询…", "还在查询…"], "先后顺序必须保持");
        assert_eq!(
            invocation.opt_sender.as_ref(),
            Some(&sender),
            "同一个通道实例应当相等"
        );
    }
}
