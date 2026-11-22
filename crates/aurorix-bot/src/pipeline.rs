use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use aurorix_api::{Anchor, ApiError, ErrorKind, Outbound, SeqBudget};
use aurorix_buttons::{FooterAction, FooterBucket, FooterDedup, FooterHub, FooterScope, Keyboard};
use aurorix_core::{Scene, Target};
use aurorix_gateway::ProtocolEvent;
use aurorix_log::{Level, Logger};
use aurorix_plugin::{
    Attachment, AtPart, CallbackBook, CallbackRequest, ImageSource, Incoming, InvocationSender,
    Media, Outgoing, Part, RateLimiter, RenderCard, ReplyKind, Router, Stage, MEDIA_IMAGE,
};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use serde_json::json;

use crate::config::BotConfig;
use crate::event::{self, EventCategory, MessageEvent};
use crate::interaction::InteractionEvent;
use crate::notice::{self, NoticeEvent};
use crate::ports::{
    InboxArchive, InboxBatch, InboxMessage, KvBackend, Rendered, Renderer,
    ReplyTransport, StatsDelta, StatsRecorder,
};

pub const MODULE: &str = "pipeline";

pub const NAME_REFRESH_TTL: Duration = Duration::from_secs(6 * 60 * 60);

pub const CARD_REPLY_KINDS: [ReplyKind; 2] = [ReplyKind::Handler, ReplyKind::ArgumentError];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ignored,
    Duplicate,
    NotCommand,
    Maintenance,
    Interaction,
    Replied,
    SendFailed,
    Notified,
}

impl Outcome {
    pub const fn label(self) -> &'static str {
        match self {
            Outcome::Ignored => "非消息事件",
            Outcome::Duplicate => "重复事件",
            Outcome::NotCommand => "未命中指令",
            Outcome::Maintenance => "维护拦截",
            Outcome::Interaction => "按钮交互",
            Outcome::Replied => "已回复",
            Outcome::SendFailed => "回复发送失败",
            Outcome::Notified => "已发送通知",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessReport {
    pub outcome: Outcome,
    pub category: EventCategory,
    pub stage: Option<Stage>,
    pub reply_text: Option<String>,
    pub sent: bool,
    pub send_error: Option<String>,
    pub rendered: bool,
    pub stats_recorded: bool,
    pub inbox_recorded: bool,
}

impl ProcessReport {
    fn base(outcome: Outcome, category: EventCategory) -> Self {
        Self {
            outcome,
            category,
            stage: None,
            reply_text: None,
            sent: false,
            send_error: None,
            rendered: false,
            stats_recorded: false,
            inbox_recorded: false,
        }
    }
}

type GuideKey = (String, String);

static GUIDE_MESSAGES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<GuideKey, (String, std::time::Instant)>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

const GUIDE_WINDOW: Duration = Duration::from_secs(1800);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineCounters {
    pub received: u64,
    pub duplicates: u64,
    pub replied: u64,
    pub send_failed: u64,
    pub ignored: u64,
    pub interactions: u64,
    pub notices: u64,
}

#[derive(Default)]
struct Counters {
    received: AtomicU64,
    duplicates: AtomicU64,
    replied: AtomicU64,
    send_failed: AtomicU64,
    ignored: AtomicU64,
    interactions: AtomicU64,
    notices: AtomicU64,
}

impl Counters {
    fn snapshot(&self) -> PipelineCounters {
        PipelineCounters {
            received: self.received.load(Ordering::SeqCst),
            duplicates: self.duplicates.load(Ordering::SeqCst),
            replied: self.replied.load(Ordering::SeqCst),
            send_failed: self.send_failed.load(Ordering::SeqCst),
            ignored: self.ignored.load(Ordering::SeqCst),
            interactions: self.interactions.load(Ordering::SeqCst),
            notices: self.notices.load(Ordering::SeqCst),
        }
    }

    fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

pub struct PipelineDeps {
    pub config: Arc<BotConfig>,
    pub router: Arc<Router>,
    pub transport: Arc<dyn ReplyTransport>,
    pub logger: Arc<Logger>,
    pub dedup_ttl: Duration,
    pub kv: Option<Arc<dyn KvBackend>>,
    pub inbox: Option<Arc<dyn InboxArchive>>,
    pub stats: Option<Arc<dyn StatsRecorder>>,
    pub renderer: Option<Arc<dyn Renderer>>,
    pub images: Option<Arc<dyn aurorix_api::ImageStore>>,
    pub footers: Option<Arc<FooterHub>>,
    pub callbacks: Option<Arc<CallbackBook>>,
    pub maintenance: Option<String>,
    pub live: Option<Arc<aurorix_admin::LiveBus>>,
}

impl PipelineDeps {
    pub fn new(
        config: Arc<BotConfig>,
        router: Arc<Router>,
        transport: Arc<dyn ReplyTransport>,
        logger: Arc<Logger>,
    ) -> Self {
        Self {
            config,
            router,
            transport,
            logger,
            dedup_ttl: aurorix_api::DEFAULT_DEDUP_TTL,
            kv: None,
            inbox: None,
            stats: None,
            renderer: None,
            images: None,
            footers: None,
            callbacks: None,
            maintenance: None,
            live: None,
        }
    }

    pub fn with_callbacks(mut self, callbacks: Arc<CallbackBook>) -> Self {
        self.callbacks = Some(callbacks);
        self
    }
}

pub struct Pipeline {
    config: Arc<BotConfig>,
    router: Arc<Router>,
    transport: Arc<dyn ReplyTransport>,
    logger: Arc<Logger>,
    dedup: aurorix_api::DedupWindow,
    kv: Option<Arc<dyn KvBackend>>,
    inbox: Option<Arc<dyn InboxArchive>>,
    stats: Option<Arc<dyn StatsRecorder>>,
    renderer: Option<Arc<dyn Renderer>>,
    images: Option<Arc<dyn aurorix_api::ImageStore>>,
    footers: Option<Arc<FooterHub>>,
    callbacks: Option<Arc<CallbackBook>>,
    maintenance: ArcSwap<Option<String>>,
    live: Option<Arc<aurorix_admin::LiveBus>>,
    name_capacity: usize,
    name_refresh: DashMap<String, std::time::Instant>,
    budget_capacity: usize,
    budgets: DashMap<String, Arc<SeqBudget>>,
    latest: DashMap<String, Anchor>,
    notice_capacity: usize,
    counters: Counters,
    notice_gate: DashMap<String, bool>,
    sender_weak: std::sync::Weak<Pipeline>,
}

impl Pipeline {
    pub fn new(deps: PipelineDeps) -> Arc<Self> {
        let name_capacity = aurorix_core::HardwareProfile::cached().name_cache_capacity();
        let budget_capacity = name_capacity.max(crate::pool::POOL_QUEUE);
        Arc::new_cyclic(|sender_weak| {
            deps.router.set_push_sink(push_sink_of(sender_weak.clone()));
            Self {
            config: deps.config,
            router: deps.router,
            transport: deps.transport,
            logger: deps.logger,
            dedup: aurorix_api::DedupWindow::new(deps.dedup_ttl),
            kv: deps.kv,
            inbox: deps.inbox,
            stats: deps.stats,
            renderer: deps.renderer,
            images: deps.images,
            footers: deps.footers,
            callbacks: deps.callbacks,
            maintenance: ArcSwap::from_pointee(deps.maintenance),
            live: deps.live,
            name_capacity,
            name_refresh: DashMap::new(),
            budget_capacity,
            budgets: DashMap::new(),
            latest: DashMap::new(),
            notice_capacity: name_capacity,
            counters: Counters::default(),
            notice_gate: DashMap::new(),
            sender_weak: sender_weak.clone(),
            }
        })
    }

    pub fn sender_for(&self, message: &MessageEvent) -> Option<InvocationSender> {
        let pipeline = self.sender_weak.upgrade()?;
        let message = message.clone();
        Some(InvocationSender::new(move |outgoing: Outgoing| {
            let pipeline = Arc::clone(&pipeline);
            let message = message.clone();
            Box::pin(async move {
                let deliver = pipeline.deliver(&message, &outgoing).await;
                pipeline
                    .record_outbound(
                        &message,
                        deliver.outbound_text.as_deref(),
                        &deliver.platform_id,
                    )
                    .await;
                match deliver.sent {
                    true => Ok(()),
                    false => Err(aurorix_plugin::PluginError::reply(
                        deliver
                            .error
                            .unwrap_or_else(|| String::from("中途消息发送失败")),
                    )),
                }
            })
        }))
    }

    pub fn remember_anchor(&self, scene: Scene, target: &Target, anchor: Anchor) {
        if anchor.empty() {
            return;
        }
        trim_cache(&self.latest, self.budget_capacity);
        self.latest
            .insert(conversation_key(scene, target), anchor);
    }

    pub fn latest_anchor(&self, scene: Scene, target: &Target) -> Anchor {
        self.latest
            .get(&conversation_key(scene, target))
            .map(|entry| entry.clone())
            .unwrap_or_default()
    }

    pub fn budget_for(&self, message: &MessageEvent) -> Arc<SeqBudget> {
        if !message.message_id.is_empty() {
            return self.budget_of(&message.message_id);
        }
        self.budget_of(&message.event_id)
    }

    pub fn budget_of(&self, key: &str) -> Arc<SeqBudget> {
        if key.is_empty() {
            return SeqBudget::shared();
        }
        enforce_budget_capacity(&self.budgets, self.budget_capacity);
        match self.budgets.entry(key.to_string()) {
            Entry::Occupied(slot) => Arc::clone(slot.get()),
            Entry::Vacant(slot) => {
                let budget = SeqBudget::shared();
                slot.insert(Arc::clone(&budget));
                budget
            }
        }
    }

    pub fn config(&self) -> &Arc<BotConfig> {
        &self.config
    }

    pub fn router(&self) -> &Arc<Router> {
        &self.router
    }

    pub fn transport(&self) -> &Arc<dyn ReplyTransport> {
        &self.transport
    }

    pub fn logger(&self) -> &Arc<Logger> {
        &self.logger
    }

    pub fn dedup(&self) -> &aurorix_api::DedupWindow {
        &self.dedup
    }

    pub fn counters(&self) -> PipelineCounters {
        self.counters.snapshot()
    }

    pub fn has_inbox(&self) -> bool {
        self.inbox.is_some()
    }

    pub fn has_stats(&self) -> bool {
        self.stats.is_some()
    }

    pub fn has_kv(&self) -> bool {
        self.kv.is_some()
    }

    pub fn footers(&self) -> Option<&Arc<FooterHub>> {
        self.footers.as_ref()
    }

    pub fn has_footers(&self) -> bool {
        self.footers.is_some()
    }

    pub fn callbacks(&self) -> &CallbackBook {
        match self.callbacks.as_deref() {
            Some(book) => book,
            None => CallbackBook::global(),
        }
    }

    pub fn has_callbacks(&self) -> bool {
        self.callbacks.is_some()
    }

    pub fn has_renderer(&self) -> bool {
        self.renderer.is_some()
    }

    pub fn maintenance(&self) -> Option<String> {
        self.maintenance.load().as_ref().clone()
    }

    pub fn set_maintenance(&self, text: Option<String>) {
        self.maintenance.store(Arc::new(text));
    }

    pub async fn close(&self) {
        if let Some(renderer) = &self.renderer {
            renderer.shutdown().await;
        }
        if let Some(inbox) = &self.inbox {
            inbox.close().await;
        }
        if let Some(stats) = &self.stats {
            stats.close().await;
        }
        if let Some(kv) = &self.kv {
            kv.close().await;
        }
    }

    pub async fn handle(&self, event: &ProtocolEvent) -> ProcessReport {
        let category = event::category(event.event_type.as_deref());
        if category == EventCategory::Interaction {
            return self.handle_interaction(event, category).await;
        }
        if let Some(notice) = NoticeEvent::parse(event) {
            return self.handle_notice(notice, category).await;
        }
        if let Some(gate) = notice::receive_gate(event) {
            self.gate_notice(&gate.group, gate.allowed);
            self.log(
                Level::Debug,
                "群消息接收开关已更新",
                vec![
                    ("conversation", gate.group),
                    ("enabled", gate.allowed.to_string()),
                ],
            );
            return ProcessReport::base(Outcome::Ignored, category);
        }
        if !category.dispatchable() {
            Counters::bump(&self.counters.ignored);
            self.log(
                Level::Debug,
                "非消息事件, 不进入指令流水线",
                vec![
                    ("event_type", event.event_type.clone().unwrap_or_default()),
                    ("category", category.label().to_string()),
                ],
            );
            return ProcessReport::base(Outcome::Ignored, category);
        }
        let Some(message) = MessageEvent::parse(event) else {
            Counters::bump(&self.counters.ignored);
            self.log(
                Level::Debug,
                "消息事件缺少必需字段或未被点名, 跳过",
                vec![
                    ("event_type", event.event_type.clone().unwrap_or_default()),
                    ("event_id", event.id.clone().unwrap_or_default()),
                ],
            );
            return ProcessReport::base(Outcome::Ignored, category);
        };
        Counters::bump(&self.counters.received);
        let (event_id, message_id) = message.dedup_key();
        if !self.dedup.check_and_mark_event(event_id, message_id) {
            Counters::bump(&self.counters.duplicates);
            self.log(
                Level::Debug,
                "重复事件已丢弃",
                vec![
                    ("event_id", event_id.to_string()),
                    ("message_id", message_id.to_string()),
                ],
            );
            return ProcessReport::base(Outcome::Duplicate, category);
        }
        self.log(
            Level::Debug,
            "收到消息",
            vec![
                ("scene", message.scene.as_str().to_string()),
                ("conversation", message.target.id.clone()),
                ("user", message.user.clone()),
                ("event_id", event_id.to_string()),
                ("message_id", message_id.to_string()),
            ],
        );
        self.remember_anchor(
            message.scene,
            &message.target,
            Anchor::new(message.message_id.clone(), message.event_id.clone()),
        );
        let incoming: Incoming = message.to_incoming();
        let outcome = self
            .router
            .dispatch_detailed_with(incoming, self.sender_for(&message))
            .await;
        let stage = outcome.stage;
        let mut report = ProcessReport::base(Outcome::NotCommand, category);
        report.stage = Some(stage);
        let mut deliver = DeliverReport::default();
        if let Some(reply) = outcome.reply {
            report.reply_text = Some(reply.body());
            deliver = self.deliver(&message, &reply).await;
            report.sent = deliver.sent;
            report.send_error = deliver.error.clone();
            report.rendered = deliver.rendered;
            report.outcome = match deliver.sent {
                true => Outcome::Replied,
                false => Outcome::SendFailed,
            };
        }
        self.log(
            Level::Debug,
            "指令分发完成",
            vec![
                ("stage", stage.label().to_string()),
                ("plugin", outcome.invocation.plugin.clone()),
                ("command", outcome.invocation.command.clone()),
                ("outcome", report.outcome.label().to_string()),
            ],
        );
        report.stats_recorded = self.record_stats(&message, deliver.sent).await;
        report.inbox_recorded = self
            .record_inbox(
                &message,
                deliver.outbound_text.as_deref(),
                &deliver.platform_id,
            )
            .await;
        report
    }

    async fn deliver(&self, message: &MessageEvent, reply: &Outgoing) -> DeliverReport {
        let keyboard = self.reply_keyboard(message.scene, &message.raw_content, reply);
        let (pieces, rendered) = self.ready_pieces(message.scene, reply).await;
        let senders = self
            .plan(&message.target, message.scene, reply, pieces, keyboard.as_ref())
            .await;
        let context = EmitContext {
            target: &message.target,
            scene: message.scene,
            anchor: Anchor::new(message.message_id.clone(), message.event_id.clone()),
            budget: Some(self.budget_for(message)),
            message_id: (!message.message_id.is_empty()).then(|| message.message_id.clone()),
            mode: Mode::Reply,
        };
        let mut report = DeliverReport {
            rendered,
            ..DeliverReport::default()
        };
        if senders.is_empty() {
            self.log(
                Level::Warn,
                context.mode.empty_label(),
                vec![("kind", format!("{:?}", reply.kind))],
            );
        }
        for sender in senders {
            report = self.emit(&context, sender, report).await;
        }
        self.track_guide(&context, reply, &report).await;
        report
    }


    async fn track_guide(
        &self,
        context: &EmitContext<'_>,
        reply: &Outgoing,
        report: &DeliverReport,
    ) {
        let user = context.target.user.clone();
        if user.is_empty() {
            return;
        }
        let key = (context.target.id.clone(), user);
        if reply.guide {
            if !report.platform_id.is_empty() {
                if let Ok(mut guards) = GUIDE_MESSAGES.lock() {
                    guards.insert(key, (report.platform_id.clone(), std::time::Instant::now()));
                }
            }
            return;
        }
        let pending = match GUIDE_MESSAGES.lock() {
            Ok(mut guards) => guards.remove(&key),
            Err(_) => None,
        };
        let Some((old, at)) = pending else {
            return;
        };
        if old.is_empty() || !report.sent || at.elapsed() > GUIDE_WINDOW {
            return;
        }
        if let Err(error) = self.transport.recall(context.target, old.as_str()).await {
            self.log(
                Level::Warn,
                "引导消息撤回失败",
                vec![("error", error.to_string())],
            );
        }
    }
    pub async fn push(&self, target: &Target, scene: Scene, reply: &Outgoing) -> PushReport {
        let keyboard = self.reply_keyboard(scene, "", reply);
        let (pieces, rendered) = self.ready_pieces(scene, reply).await;
        let senders = self.plan(target, scene, reply, pieces, keyboard.as_ref()).await;
        let anchor = self.latest_anchor(scene, target);
        let budget = match anchor.empty() {
            true => None,
            false => Some(self.budget_of(&anchor.msg_id)),
        };
        let message_id = (!anchor.msg_id.is_empty()).then(|| anchor.msg_id.clone());
        let context = EmitContext {
            target,
            scene,
            anchor,
            budget,
            message_id,
            mode: Mode::Push,
        };
        let mut report = DeliverReport {
            rendered,
            ..DeliverReport::default()
        };
        if senders.is_empty() {
            self.log(
                Level::Warn,
                context.mode.empty_label(),
                vec![("conversation", target.id.clone())],
            );
        }
        for sender in senders {
            report = self.emit(&context, sender, report).await;
        }
        PushReport::from_deliver(report)
    }

    async fn ready_pieces(&self, _scene: Scene, reply: &Outgoing) -> (Vec<Piece>, bool) {
        let mut rendered = false;
        let mut cards: Vec<(usize, RenderCard, Option<String>)> = Vec::new();
        let mut slots: Vec<Option<Piece>> = Vec::with_capacity(reply.parts.len());
        for part in &reply.parts {
            let piece = match part {
                Part::Text(text) => Some(Piece::Text(text.clone())),
                Part::Markdown(markdown) => Some(Piece::Markdown(markdown.clone())),
                Part::At(at) => Some(Piece::At(at.clone())),
                Part::Image(source) => match source.is_empty() {
                    true => None,
                    false => {
                        rendered |= matches!(source, ImageSource::Bytes(_));
                        Some(Piece::Image(source.clone()))
                    }
                },
                Part::Media { kind, media } => match media.is_empty() {
                    true => None,
                    false => Some(Piece::Media {
                        kind: *kind,
                        media: media.clone(),
                    }),
                },
                Part::Card { card, fallback } => {
                    cards.push((slots.len(), card.clone(), fallback.clone()));
                    None
                }
            };
            slots.push(piece);
        }
        if !cards.is_empty() {
            let (card_rendered, fell_back) = self.render_cards(cards, &mut slots).await;
            rendered |= card_rendered;
            for index in fell_back {
                for slot in slots.iter_mut().take(index) {
                    if let Some(Piece::Text(_)) = slot {
                        *slot = None;
                    }
                }
            }
        }
        let pieces: Vec<Piece> = slots.into_iter().flatten().collect();
        (pieces, rendered)
    }

    async fn render_cards(
        &self,
        cards: Vec<(usize, RenderCard, Option<String>)>,
        slots: &mut [Option<Piece>],
    ) -> (bool, Vec<usize>) {
        let mut rendered = false;
        let mut fell_back: Vec<usize> = Vec::new();
        let Some(renderer) = self.renderer.clone() else {
            for (index, card, fallback) in cards {
                self.log(
                    Level::Warn,
                    "没有可用的渲染器, 卡片回退纯文本",
                    vec![("label", card.label.clone())],
                );
                if fallback.is_some() {
                    fell_back.push(index);
                }
                slots[index] = fallback.map(Piece::Text);
            }
            return (rendered, fell_back);
        };
        let mut tasks = tokio::task::JoinSet::new();
        for (index, card, fallback) in cards {
            let renderer = Arc::clone(&renderer);
            let label = card.label.clone();
            tasks.spawn(async move { (index, label, fallback, renderer.render_card(&card).await) });
        }
        while let Some(joined) = tasks.join_next().await {
            let (index, label, fallback, outcome) = match joined {
                Ok(value) => value,
                Err(error) => {
                    self.log(
                        Level::Warn,
                        "卡片渲染任务中断",
                        vec![("error", error.to_string())],
                    );
                    continue;
                }
            };
            match outcome {
                Ok(Rendered::Image { bytes, filename }) => {
                    let mime = mime_of(&filename);
                    slots[index] = Some(Piece::Image(ImageSource::Bytes(Attachment::new(
                        bytes, filename, mime,
                    ))));
                    rendered = true;
                }
                Ok(Rendered::Text(text)) => {
                    slots[index] = Some(Piece::Text(text));
                    rendered = true;
                }
                Err(error) => {
                    self.log(
                        Level::Warn,
                        "卡片渲染失败, 回退纯文本回复",
                        vec![("label", label), ("error", error.to_string())],
                    );
                    if fallback.is_some() {
                        fell_back.push(index);
                    }
                    slots[index] = fallback.map(Piece::Text);
                }
            }
        }
        (rendered, fell_back)
    }

    async fn plan(
        &self,
        target: &Target,
        scene: Scene,
        reply: &Outgoing,
        pieces: Vec<Piece>,
        keyboard: Option<&Keyboard>,
    ) -> Vec<Sender> {
        match scene.guild() {
            true => self.plan_guild(target, scene, reply, pieces),
            false => self.plan_chat(target, scene, reply, pieces, keyboard).await,
        }
    }

    async fn plan_chat(
        &self,
        target: &Target,
        scene: Scene,
        reply: &Outgoing,
        pieces: Vec<Piece>,
        keyboard: Option<&Keyboard>,
    ) -> Vec<Sender> {
        let mut use_markdown = keyboard.is_some();
        let (mut has_text, mut has_image) = (false, false);
        for piece in &pieces {
            match piece {
                Piece::Markdown(_) => use_markdown = true,
                Piece::Text(text) => {
                    has_text = true;
                    use_markdown |= has_markdown_markup(text);
                }
                Piece::Image(_) => has_image = true,
                Piece::At(AtPart::Avatar { .. }) => use_markdown = true,
                _ => {}
            }
        }
        if self.config.global_markdown && (has_text || has_image) {
            use_markdown = true;
        }
        let fragments = match use_markdown {
            true => self.image_fragments(target, &pieces).await,
            false => Vec::new(),
        };
        let mut inline = String::new();
        let mut media: Vec<Sender> = Vec::new();
        let mut next = 0usize;
        for piece in pieces {
            match piece {
                Piece::Text(text) => inline.push_str(&text),
                Piece::Markdown(markdown) => push_markdown(&mut inline, &markdown),
                Piece::At(at) => {
                    inline.push_str(&self.at_markup(scene, &at, use_markdown));
                    if at.newline() {
                        inline.push('\n');
                    }
                }
                Piece::Image(source) => {
                    let fragment = match use_markdown {
                        true => {
                            let slot = fragments.get(next).cloned().flatten();
                            next += 1;
                            slot
                        }
                        false => None,
                    };
                    match fragment {
                        Some(fragment) => inline.push_str(fragment.trim_end_matches('\n')),
                        None => media.push(Sender::Media {
                            kind: MEDIA_IMAGE,
                            media: image_media(&source),
                        }),
                    }
                }
                Piece::Media { kind, media: source } => media.push(Sender::Media {
                    kind,
                    media: source,
                }),
            }
        }
        let mut content = format!(
            "{}{inline}",
            self.mention_prefix(scene, target, reply, use_markdown)
        );
        let mut out: Vec<Sender> = Vec::new();
        if use_markdown && (!content.is_empty() || keyboard.is_some()) {
            content = self.prepare_content(target, scene, content).await;
            if content.trim().is_empty() {
                content = String::from(" ");
            }
            let mut outbound = self.apply_keyboard(Outbound::markdown(content), keyboard);
            if let Some(quote) = quote_of(reply) {
                outbound = outbound.quote_to(quote);
            }
            out.push(Sender::Message(outbound));
        } else if !content.is_empty() {
            content = self.prepare_content(target, scene, content).await;
            let mut outbound = Outbound::text(content);
            if let Some(quote) = quote_of(reply) {
                outbound = outbound.quote_to(quote);
            }
            out.push(Sender::Message(outbound));
        }
        out.extend(media);
        out
    }

    async fn image_fragments(&self, target: &Target, pieces: &[Piece]) -> Vec<Option<String>> {
        let sources: Vec<ImageSource> = pieces
            .iter()
            .filter_map(|piece| match piece {
                Piece::Image(source) => Some(source.clone()),
                _ => None,
            })
            .collect();
        let mut slots: Vec<Option<String>> = vec![None; sources.len()];
        if sources.is_empty() {
            return slots;
        }
        let mut tasks = tokio::task::JoinSet::new();
        for (index, source) in sources.into_iter().enumerate() {
            let store = self.images.clone();
            let target = target.clone();
            tasks.spawn(async move { (index, image_fragment(store, target, source).await) });
        }
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok((index, Ok(fragment))) => slots[index] = Some(fragment),
                Ok((_, Err(reason))) => self.log(
                    Level::Warn,
                    "图片内嵌失败, 改走媒体上传",
                    vec![("reason", reason)],
                ),
                Err(error) => self.log(
                    Level::Warn,
                    "图片解析任务中断",
                    vec![("error", error.to_string())],
                ),
            }
        }
        slots
    }

    fn plan_guild(
        &self,
        target: &Target,
        scene: Scene,
        reply: &Outgoing,
        pieces: Vec<Piece>,
    ) -> Vec<Sender> {
        let mut inline = String::new();
        let mut media: Vec<Sender> = Vec::new();
        for piece in pieces {
            match piece {
                Piece::Text(text) => inline.push_str(&text),
                Piece::Markdown(markdown) => push_markdown(&mut inline, &markdown),
                Piece::At(at) => {
                    inline.push_str(&self.at_markup(scene, &at, false));
                    if at.newline() {
                        inline.push('\n');
                    }
                }
                Piece::Image(source) => media.push(guild_image(source)),
                Piece::Media { kind, media: source } => media.push(Sender::Media {
                    kind,
                    media: source,
                }),
            }
        }
        let (_, rest) = aurorix_api::text::split_images(&inline);
        let mut caption = aurorix_api::text::guild_text(&rest);
        if reply.image_only && !media.is_empty() {
            caption.clear();
        } else {
            caption = format!("{}{caption}", self.mention_prefix(scene, target, reply, false));
        }
        if !caption.trim().is_empty() {
            match media.first_mut() {
                Some(Sender::Image { caption: slot, .. })
                | Some(Sender::ChannelImage { caption: slot, .. }) => {
                    *slot = caption;
                    caption = String::new();
                }
                _ => {}
            }
        }
        let mut out: Vec<Sender> = Vec::new();
        if !caption.trim().is_empty() {
            let mut outbound = Outbound::text(caption);
            if let Some(quote) = quote_of(reply) {
                outbound = outbound.quote_to(quote);
            }
            out.push(Sender::Message(outbound));
        }
        out.extend(media);
        out
    }

    async fn emit(
        &self,
        context: &EmitContext<'_>,
        sender: Sender,
        report: DeliverReport,
    ) -> DeliverReport {
        match sender {
            Sender::Message(outbound) => self.emit_message(context, outbound, report).await,
            Sender::Image {
                caption,
                attachment,
            } => self.emit_image(context, caption, attachment, report).await,
            Sender::ChannelImage { caption, url } => {
                self.emit_channel_image(context, caption, url, report).await
            }
            Sender::Media { kind, media } => self.emit_media(context, kind, &media, report).await,
        }
    }

    async fn emit_message(
        &self,
        context: &EmitContext<'_>,
        outbound: Outbound,
        report: DeliverReport,
    ) -> DeliverReport {
        if outbound.content.trim().is_empty()
            && outbound.media.is_none()
            && outbound.keyboard.is_none()
        {
            self.log(
                Level::Warn,
                context.mode.empty_label(),
                vec![("conversation", context.target.id.clone())],
            );
            return report;
        }
        let text = outbound.content.clone();
        self.send_outbound(context, outbound, Some(text), report)
            .await
    }

    async fn emit_image(
        &self,
        context: &EmitContext<'_>,
        caption: String,
        attachment: Attachment,
        mut report: DeliverReport,
    ) -> DeliverReport {
        report.rendered = true;
        let sent = self
            .transport
            .send_image_bytes(
                context.target,
                caption,
                attachment.data.clone(),
                attachment.filename.clone(),
                attachment.mime_type.clone(),
                context.message_id.clone(),
            )
            .await;
        match sent {
            Ok(id) => {
                Counters::bump(&self.counters.replied);
                report.sent = true;
                report.platform_id = id.unwrap_or_default();
                report.outbound_text = Some(String::from("[频道图片]"));
                let bytes = attachment.data.len().to_string();
                self.log(
                    Level::Info,
                    context.mode.image_label(),
                    vec![
                        ("conversation", context.target.id.clone()),
                        ("message_id", report.platform_id.clone()),
                        ("bytes", bytes),
                    ],
                );
                self.archive(context, &mut report).await;
            }
            Err(error) => self.note_failure(context, &error, &mut report),
        }
        report
    }

    async fn emit_channel_image(
        &self,
        context: &EmitContext<'_>,
        caption: String,
        url: String,
        mut report: DeliverReport,
    ) -> DeliverReport {
        report.rendered = true;
        let sent = self
            .transport
            .send_channel_image(
                context.target,
                caption,
                url.clone(),
                context.message_id.clone(),
            )
            .await;
        match sent {
            Ok(id) => {
                Counters::bump(&self.counters.replied);
                report.sent = true;
                report.platform_id = id.unwrap_or_default();
                report.outbound_text = Some(String::from("[频道图片]"));
                self.log(
                    Level::Info,
                    context.mode.image_label(),
                    vec![
                        ("conversation", context.target.id.clone()),
                        ("message_id", report.platform_id.clone()),
                        ("url", url),
                    ],
                );
                self.archive(context, &mut report).await;
            }
            Err(error) => self.note_failure(context, &error, &mut report),
        }
        report
    }

    async fn emit_media(
        &self,
        context: &EmitContext<'_>,
        kind: i32,
        media: &Media,
        mut report: DeliverReport,
    ) -> DeliverReport {
        if kind == MEDIA_IMAGE {
            report.rendered = true;
        }
        let uploaded = match self
            .upload_media(context.target, context.scene, kind, media)
            .await
        {
            Ok(uploaded) => uploaded,
            Err(error) => {
                self.log(
                    Level::Warn,
                    "媒体上传失败, 跳过该部件",
                    vec![
                        ("file_type", kind.to_string()),
                        ("error", error.to_string()),
                    ],
                );
                return report;
            }
        };
        let outbound = Outbound::media(
            String::new(),
            json!({ "file_info": uploaded.file_info }),
        );
        let text = match kind == MEDIA_IMAGE {
            true => Some(String::from("[卡片图片]")),
            false => None,
        };
        self.send_outbound(context, outbound, text, report).await
    }

    async fn send_outbound(
        &self,
        context: &EmitContext<'_>,
        outbound: Outbound,
        text: Option<String>,
        mut report: DeliverReport,
    ) -> DeliverReport {
        let mut sealed = outbound;
        if let Some(budget) = context.budget.as_ref() {
            sealed = sealed
                .with_anchor(context.anchor.clone())
                .with_budget(Arc::clone(budget));
        }
        if matches!(context.mode, Mode::Push) && context.anchor.empty() {
            sealed = sealed.as_initiative();
        }
        if let Some(text) = text {
            report.outbound_text = Some(text);
        }
        match self.transport.send(context.target, &sealed).await {
            Ok(outcome) => {
                Counters::bump(&self.counters.replied);
                report.sent = true;
                report.platform_id = outcome.message_id.unwrap_or_default();
                self.log(
                    Level::Info,
                    context.mode.sent_label(),
                    vec![
                        ("scene", context.scene.as_str().to_string()),
                        ("conversation", context.target.id.clone()),
                        ("message_id", report.platform_id.clone()),
                        ("active", outcome.delivered_active.to_string()),
                        ("fallback", outcome.fallback.to_string()),
                    ],
                );
                self.archive(context, &mut report).await;
            }
            Err(error) => self.note_failure(context, &error, &mut report),
        }
        report
    }

    async fn archive(&self, context: &EmitContext<'_>, report: &mut DeliverReport) {
        if !matches!(context.mode, Mode::Push) {
            return;
        }
        let text = report.outbound_text.clone().unwrap_or_default();
        report.archived = self
            .record_push(context.target, context.scene, text.as_str(), &report.platform_id)
            .await;
    }

    fn note_failure(&self, context: &EmitContext<'_>, error: &ApiError, report: &mut DeliverReport) {
        Counters::bump(&self.counters.send_failed);
        let (label, level) = classify_send_error(error);
        report.error = Some(format!("{label}: {error}"));
        self.log(
            level,
            context.mode.failed_label(),
            vec![
                ("reason", label.to_string()),
                ("error", error.to_string()),
                ("scene", context.scene.as_str().to_string()),
                ("conversation", context.target.id.clone()),
                (
                    "code",
                    error
                        .code()
                        .map(|code| code.to_string())
                        .unwrap_or_default(),
                ),
                (
                    "kind",
                    error
                        .kind()
                        .map(|kind| kind.label().to_string())
                        .unwrap_or_else(|| String::from("非平台错误")),
                ),
            ],
        );
    }

    async fn upload_media(
        &self,
        target: &Target,
        scene: Scene,
        kind: i32,
        media: &Media,
    ) -> Result<aurorix_api::MediaResult, ApiError> {
        if kind == MEDIA_IMAGE {
            if let Media::Bytes { data, filename } = media {
                return self
                    .transport
                    .upload_image(target, data.clone(), filename.clone())
                    .await;
            }
        }
        let mut upload = crate::media::upload_for(kind, media)?;
        let mut link = upload.url.clone();
        if !scene.guild()
            && kind != aurorix_api::MEDIA_FILE
            && link.is_none()
            && !upload.data.is_empty()
            && (upload.data.len() > self.config.upload_threshold
                || kind == aurorix_api::MEDIA_VIDEO)
        {
            if let Some(store) = &self.images {
                let started = std::time::Instant::now();
                match store
                    .publish(&upload.data, upload.file_type, upload.filename.as_str())
                    .await
                {
                    Ok(url) => {
                        self.log(
                            Level::Info,
                            "媒体转存图床完成",
                            vec![
                                ("file_type", kind.to_string()),
                                ("bytes", upload.data.len().to_string()),
                                ("ms", started.elapsed().as_millis().to_string()),
                            ],
                        );
                        link = Some(url);
                    }
                    Err(error) => self.log(
                        Level::Warn,
                        "媒体转存图床失败, 改走直传",
                        vec![
                            ("file_type", kind.to_string()),
                            ("error", error.to_string()),
                        ],
                    ),
                }
            }
        }
        upload.url = link.clone();
        let sent_at = std::time::Instant::now();
        let mut outcome = self.transport.upload_media(target, &upload).await;
        self.log(
            match outcome.is_ok() {
                true => Level::Info,
                false => Level::Warn,
            },
            "媒体发送完成",
            vec![
                ("file_type", kind.to_string()),
                ("bytes", upload.data.len().to_string()),
                ("link", link.is_some().to_string()),
                ("ms", sent_at.elapsed().as_millis().to_string()),
            ],
        );
        if outcome.is_err() && link.is_some() && !upload.data.is_empty() {
            self.log(
                Level::Warn,
                "直链没通, 改走直传",
                vec![("file_type", kind.to_string())],
            );
            upload.url = None;
            outcome = self.transport.upload_media(target, &upload).await;
        }
        outcome
    }

    async fn prepare_content(&self, target: &Target, scene: Scene, content: String) -> String {
        let Some(store) = &self.images else {
            return content;
        };
        if scene.guild() || content.is_empty() {
            return content;
        }
        let prepared = crate::images::with_target(
            target.clone(),
            aurorix_api::prepare_with(&content, Some(store.as_ref()), 0, None),
        )
        .await;
        for image in &prepared.images {
            if let Some(failure) = &image.failure {
                self.log(
                    Level::Warn,
                    "图床替换失败, 保留原图地址",
                    vec![
                        ("source", image.source.clone()),
                        ("reason", failure.clone()),
                    ],
                );
            }
        }
        prepared.content
    }

    fn apply_keyboard(&self, outbound: Outbound, keyboard: Option<&Keyboard>) -> Outbound {
        let Some(keyboard) = keyboard else {
            return outbound;
        };
        match keyboard.to_json() {
            Ok(raw) => match serde_json::from_slice(&raw) {
                Ok(value) => outbound.with_keyboard_json(value),
                Err(error) => {
                    self.log(
                        Level::Warn,
                        "键盘解析失败, 改为只发文本",
                        vec![("error", error.to_string())],
                    );
                    outbound
                }
            },
            Err(error) => {
                self.log(
                    Level::Warn,
                    "键盘构建失败, 改为只发文本",
                    vec![("error", error.to_string())],
                );
                outbound
            }
        }
    }

    fn mention_prefix(
        &self,
        scene: Scene,
        target: &Target,
        reply: &Outgoing,
        markdown: bool,
    ) -> String {
        let mut prefix = String::new();
        let body = reply.body();
        let mentioned = reply
            .mention_user
            .as_deref()
            .map(str::trim)
            .filter(|user| !user.is_empty());
        if let Some(user) = mentioned {
            if !already_mentions(&body, user) {
                prefix.push_str(&self.at_prefix(scene, user, markdown));
            }
        }
        if reply.at {
            let user = target.mention();
            if !already_mentions(&body, user) && !already_mentions(&prefix, user) {
                prefix.push_str(&self.at_prefix(scene, user, markdown));
            }
        }
        prefix
    }

    fn at_markup(&self, scene: Scene, at: &AtPart, markdown: bool) -> String {
        match at {
            AtPart::All { .. } => match markdown {
                true => String::from("<qqbot-at-everyone />"),
                false => String::from("@everyone"),
            },
            AtPart::Mention { openid, .. } => match markdown {
                true => event::markdown_mention(scene, openid),
                false => event::mention_tag(scene, openid),
            },
            AtPart::Avatar { openid, size } => {
                let mut markup = String::new();
                if markdown && !self.config.appid.is_empty() {
                    markup.push_str(&format!(
                        "![cornerRadius=100 #{size}px #{size}px]({}) ",
                        event::avatar_url(&self.config.appid, openid, event::AT_AVATAR_FETCH_PX)
                    ));
                }
                markup.push_str(&match markdown {
                    true => event::markdown_mention(scene, openid),
                    false => event::mention_tag(scene, openid),
                });
                markup
            }
        }
    }

    fn at_prefix(&self, scene: Scene, user: &str, markdown: bool) -> String {
        if user.trim().is_empty() {
            return String::new();
        }
        match markdown && !self.config.appid.is_empty() {
            true => event::avatar_mention(scene, &self.config.appid, user),
            false => event::mention_tag(scene, user),
        }
    }

    fn reply_keyboard(&self, scene: Scene, raw_content: &str, reply: &Outgoing) -> Option<Keyboard> {
        if reply.no_footer {
            return reply.keyboard.clone();
        }
        let Some(footers) = self.footers.as_ref() else {
            return reply.keyboard.clone();
        };
        footers.merge(reply.keyboard.clone(), raw_content, scene)
    }

    async fn record_push(
        &self,
        target: &Target,
        scene: Scene,
        text: &str,
        platform_id: &str,
    ) -> bool {
        let Some(inbox) = &self.inbox else {
            return false;
        };
        let mut batch = InboxBatch::default();
        batch.push(InboxMessage::outbound(
            target.id.clone(),
            scene,
            text,
            platform_id.to_string(),
        ));
        match inbox.save(batch).await {
            Ok(()) => true,
            Err(error) => {
                self.log(
                    Level::Warn,
                    "主动消息归档失败",
                    vec![("error", error), ("conversation", target.id.clone())],
                );
                false
            }
        }
    }
    async fn handle_notice(&self, notice: NoticeEvent, category: EventCategory) -> ProcessReport {
        Counters::bump(&self.counters.notices);
        let added = notice.kind.added();
        let (enabled, lines, extra, scope) = match added {
            true => (
                self.config.welcome_on(),
                self.config.welcome_lines(),
                self.config.welcome_extra_text().to_string(),
                FooterScope::NoticeWelcome,
            ),
            false => (
                self.config.farewell_on(),
                self.config.farewell_lines(),
                self.config.farewell_extra_text().to_string(),
                FooterScope::NoticeFarewell,
            ),
        };
        let mut report = ProcessReport::base(Outcome::Notified, category);
        let kind = notice.kind.label();
        if !enabled {
            self.log(
                Level::Debug,
                "成员通知开关已关闭, 跳过",
                vec![("kind", kind.to_string())],
            );
            report.outcome = Outcome::Ignored;
            return report;
        }
        let text = notice::pick_line(&lines);
        if text.is_empty() {
            self.log(
                Level::Debug,
                "成员通知文案为空, 跳过",
                vec![("kind", kind.to_string())],
            );
            report.outcome = Outcome::Ignored;
            return report;
        }
        if !notice.can_reply() {
            if let Some(allowed) = self.notice_gate.get(&notice.target.id).map(|flag| *flag) {
                if !allowed {
                    self.log(
                        Level::Debug,
                        "会话未开启主动消息, 跳过成员通知",
                        vec![
                            ("kind", kind.to_string()),
                            ("conversation", notice.target.id.clone()),
                        ],
                    );
                    report.outcome = Outcome::Ignored;
                    return report;
                }
            }
        }
        let avatar = notice::avatar_url(&self.config.appid, &notice.user, notice::AVATAR_SIZE)
            .unwrap_or_default();
        let content = notice::compose(&avatar, &notice.user, added, &text, &extra);
        let content = self.prepare_notice_content(&notice, content).await;
        let content = aurorix_api::markdown::encode_cmd_tags(&content);
        let keyboard = self
            .footers
            .as_ref()
            .and_then(|hub| hub.keyboard(scope, ""))
            .filter(|keyboard| keyboard.to_json().is_ok());
        let mut outbound = Outbound::markdown(content.clone());
        if let Some(keyboard) = &keyboard {
            match outbound.with_keyboard(keyboard) {
                Ok(with_keyboard) => outbound = with_keyboard,
                Err(error) => {
                    self.log(
                        Level::Warn,
                        "通知键盘构建失败, 改为只发文本",
                        vec![("error", error.to_string())],
                    );
                    outbound = Outbound::markdown(content);
                }
            }
        }
        if notice.can_reply() {
            outbound = outbound.with_event_id(notice.event_id.clone());
        } else {
            outbound = outbound.as_initiative();
        }
        report.reply_text = Some(outbound.content.clone());
        self.log(
            Level::Debug,
            "准备发送成员通知",
            vec![
                ("kind", kind.to_string()),
                ("conversation", notice.target.id.clone()),
                ("member", notice.user.clone()),
                ("passive", notice.can_reply().to_string()),
                ("keyboard", keyboard.is_some().to_string()),
            ],
        );
        match self.transport.send(&notice.target, &outbound).await {
            Ok(outcome) => {
                Counters::bump(&self.counters.replied);
                report.sent = true;
                let platform_id = outcome.message_id.unwrap_or_default();
                self.log(
                    Level::Info,
                    "成员通知已发送",
                    vec![
                        ("kind", kind.to_string()),
                        ("conversation", notice.target.id.clone()),
                        ("message_id", platform_id.clone()),
                        ("active", outcome.delivered_active.to_string()),
                    ],
                );
                report.inbox_recorded = self
                    .record_notice(&notice, &outbound.content, &platform_id)
                    .await;
            }
            Err(error) => {
                Counters::bump(&self.counters.send_failed);
                let (label, level) = classify_send_error(&error);
                if error.code() == Some(notice::NO_PERMISSION_CODE) {
                    self.gate_notice(&notice.target.id, false);
                }
                report.outcome = Outcome::SendFailed;
                report.send_error = Some(format!("{label}: {error}"));
                self.log(
                    level,
                    "成员通知发送失败",
                    vec![
                        ("kind", kind.to_string()),
                        ("reason", label.to_string()),
                        ("error", error.to_string()),
                        ("conversation", notice.target.id.clone()),
                        (
                            "code",
                            error
                                .code()
                                .map(|code| code.to_string())
                                .unwrap_or_default(),
                        ),
                    ],
                );
            }
        }
        report
    }

    async fn prepare_notice_content(&self, notice: &NoticeEvent, content: String) -> String {
        let Some(store) = &self.images else {
            return content;
        };
        if notice.scene.guild() || content.is_empty() {
            return content;
        }
        let prepared = crate::images::with_target(
            notice.target.clone(),
            aurorix_api::prepare_with(&content, Some(store.as_ref()), 0, None),
        )
        .await;
        for image in &prepared.images {
            if let Some(failure) = &image.failure {
                self.log(
                    Level::Warn,
                    "通知里的图片转存失败, 保留原地址",
                    vec![
                        ("source", image.source.clone()),
                        ("reason", failure.clone()),
                    ],
                );
            }
        }
        prepared.content
    }

    async fn record_notice(&self, notice: &NoticeEvent, text: &str, platform_id: &str) -> bool {
        let Some(inbox) = &self.inbox else {
            return false;
        };
        let mut batch = InboxBatch::default();
        batch.push(InboxMessage::outbound(
            notice.target.id.clone(),
            notice.scene,
            text,
            platform_id.to_string(),
        ));
        match inbox.save(batch).await {
            Ok(()) => true,
            Err(error) => {
                self.log(
                    Level::Warn,
                    "成员通知归档失败",
                    vec![("error", error), ("conversation", notice.target.id.clone())],
                );
                false
            }
        }
    }

    async fn handle_interaction(
        &self,
        event: &ProtocolEvent,
        category: EventCategory,
    ) -> ProcessReport {
        let ack_id = crate::interaction::interaction_id(event);
        if !ack_id.is_empty() {
            if let Err(error) = self.transport.ack_interaction(&ack_id).await {
                self.log(
                    Level::Warn,
                    "按钮交互回执失败",
                    vec![
                        ("interaction", ack_id.clone()),
                        ("error", error.to_string()),
                    ],
                );
            }
        }
        let Some(interaction) = InteractionEvent::parse(event) else {
            Counters::bump(&self.counters.ignored);
            self.log(
                Level::Warn,
                "按钮交互缺少按钮信息, 跳过",
                vec![
                    ("event_id", event.id.clone().unwrap_or_default()),
                    ("raw", clip(&event.data_text(), 800)),
                ],
            );
            return ProcessReport::base(Outcome::Ignored, category);
        };
        Counters::bump(&self.counters.interactions);
        self.log(
            Level::Debug,
            "收到按钮交互",
            vec![
                ("button", interaction.button_id.clone()),
                ("data", interaction.button_data.clone()),
                ("user", interaction.user.clone()),
                ("scene", interaction.scene.as_str().to_string()),
            ],
        );
        let mut report = ProcessReport::base(Outcome::Interaction, category);
        if let Some(text) = self.router.ban().hit(&interaction.user) {
            self.log(
                Level::Debug,
                "被封禁用户的按钮交互已拦截",
                vec![
                    ("button", interaction.button_id.clone()),
                    ("user", interaction.user.clone()),
                    ("conversation", interaction.target.id.clone()),
                ],
            );
            report.stage = Some(Stage::Banned);
            let reply = banned_interaction(&interaction, &text);
            return self.deliver_interaction(&interaction, &reply, report).await;
        }
        if let Err(wait) = self.router.limiter().click_allowed(
            &interaction.button_id,
            &interaction.target.id,
            &interaction.user,
        ) {
            self.log(
                Level::Debug,
                "按钮点击触发限流",
                vec![
                    ("button", interaction.button_id.clone()),
                    ("user", interaction.user.clone()),
                    ("wait_seconds", RateLimiter::wait_seconds(wait).to_string()),
                ],
            );
            let reply = rate_limited_interaction(&interaction, wait);
            return self.deliver_interaction(&interaction, &reply, report).await;
        }
        let outcome = match self.maintenance() {
            Some(text) => Some(Outgoing::new(ReplyKind::Maintenance, text).no_footer()),
            None => match self.footer_click_reply(&interaction).await {
                Ok(Some(reply)) => Some(reply),
                Ok(None) => self.callback_reply(&interaction).await,
                Err(error) => match self.callback_reply(&interaction).await {
                    Some(reply) => Some(reply),
                    None => {
                        self.log(
                            Level::Warn,
                            "按钮交互处理失败",
                            vec![("button", interaction.button_id.clone()), ("error", error)],
                        );
                        None
                    }
                },
            },
        };
        let Some(reply) = outcome else {
            return report;
        };
        self.deliver_interaction(&interaction, &reply, report).await
    }

    async fn deliver_interaction(
        &self,
        interaction: &InteractionEvent,
        reply: &Outgoing,
        mut report: ProcessReport,
    ) -> ProcessReport {
        let message = interaction_message(interaction);
        report.reply_text = Some(reply.body());
        let deliver = self.deliver(&message, reply).await;
        report.sent = deliver.sent;
        report.send_error = deliver.error;
        report.rendered = deliver.rendered;
        report.outcome = match deliver.sent {
            true => Outcome::Replied,
            false => Outcome::SendFailed,
        };
        report
    }

    async fn footer_click_reply(
        &self,
        interaction: &InteractionEvent,
    ) -> Result<Option<Outgoing>, String> {
        let Some(footers) = self.footers.as_ref() else {
            return Ok(None);
        };
        let click = match footers.resolve_click(&interaction.button_data) {
            Ok(click) => click,
            Err(primary) => {
                let slot = interaction
                    .button_id
                    .rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_string();
                match footers.resolve_click(&slot) {
                    Ok(click) => click,
                    Err(_) => return Err(primary.to_string()),
                }
            }
        };
        match click.action {
            FooterAction::Counter {
                count_key,
                dedup,
                bucket,
                reply,
                duplicate_reply,
                label,
            } => {
                let text = self
                    .footer_counter(
                        interaction,
                        FooterCounterInput {
                            count_key,
                            dedup,
                            bucket,
                            reply,
                            duplicate_reply,
                            label,
                        },
                    )
                    .await?;
                Ok(Some(Outgoing::new(ReplyKind::Handler, text).no_footer()))
            }
            FooterAction::Image { url, label } => Ok(Some(
                Outgoing::new(ReplyKind::Handler, format!("![{label}]({url})")).no_footer(),
            )),
            FooterAction::Command
            | FooterAction::Repeat
            | FooterAction::Link
            | FooterAction::Callback => {
                self.log(
                    Level::Debug,
                    "底部按钮由客户端或回调处理, 服务端不回复",
                    vec![("button", interaction.button_id.clone())],
                );
                Ok(None)
            }
        }
    }

    async fn callback_reply(&self, interaction: &InteractionEvent) -> Option<Outgoing> {
        let book = self.callbacks();
        let plugin = book.plugin_of(&interaction.button_id).unwrap_or_default();
        let request = CallbackRequest {
            button_id: interaction.button_id.clone(),
            data: interaction.button_data.clone(),
            plugin: plugin.clone(),
            user: interaction.user.clone(),
            scene: interaction.scene,
            target: interaction.target.clone(),
            sender: self.sender_for(&interaction_message(interaction)),
            interaction_id: interaction.interaction_id.clone(),
        };
        let future = book.dispatch(request)?;
        self.log(
            Level::Debug,
            "插件回调按钮已分派",
            vec![
                ("button", interaction.button_id.clone()),
                ("plugin", plugin),
            ],
        );
        let reply = future.await;
        if reply.is_none() {
            self.log(
                Level::Debug,
                "插件回调按钮没有回复",
                vec![("button", interaction.button_id.clone())],
            );
        }
        reply
    }

    async fn footer_counter(
        &self,
        interaction: &InteractionEvent,
        input: FooterCounterInput,
    ) -> Result<String, String> {
        let Some(kv) = self.kv.as_ref() else {
            return Err(String::from("底部按钮计数失败: 没有可用的存储命名空间"));
        };
        let bucket_id = match input.bucket {
            FooterBucket::Global => String::from("global"),
            FooterBucket::Group => interaction.target.id.clone(),
        };
        let key = format!("{}@{bucket_id}", input.count_key);
        let duplicate = match input.dedup {
            FooterDedup::None => false,
            FooterDedup::Group => mark_once(kv.as_ref(), &key).await?,
            FooterDedup::User => {
                if interaction.user.is_empty() {
                    self.log(
                        Level::Warn,
                        "底部按钮想按用户去重, 但本次点击没有用户标识, 按不去重处理",
                        vec![("key", key.clone())],
                    );
                    false
                } else {
                    mark_once(kv.as_ref(), &format!("{key}.seen.{}", interaction.user)).await?
                }
            }
        };
        let count = if duplicate {
            match kv.get(&key).await? {
                Some(value) => value.as_i64().unwrap_or(0),
                None => 0,
            }
        } else {
            kv.incr(&key, 1).await?
        };
        let template = match (duplicate, input.duplicate_reply) {
            (true, Some(text)) if !text.trim().is_empty() => text,
            _ => input.reply,
        };
        let template = match template.trim().is_empty() {
            true => String::from(aurorix_buttons::FOOTER_DEFAULT_REPLY),
            false => template,
        };
        Ok(render_footer_template(
            &template,
            count,
            &interaction.target,
            &interaction.user,
            &input.label,
        ))
    }

    async fn record_stats(&self, message: &MessageEvent, sent: bool) -> bool {
        let Some(stats) = &self.stats else {
            return false;
        };
        let delta = StatsDelta {
            scene: message.scene,
            conversation: message.target.id.clone(),
            name: match message.scene {
                Scene::C2c => message.username.clone().unwrap_or_default(),
                _ => String::new(),
            },
            received: 1,
            sent: u64::from(sent),
            button: 0,
            day: aurorix_db::stats::local_day_key(chrono::Local::now()),
        };
        match stats.record(delta).await {
            Ok(()) => true,
            Err(error) => {
                self.log(
                    Level::Warn,
                    "统计写入失败",
                    vec![
                        ("error", error),
                        ("conversation", message.target.id.clone()),
                    ],
                );
                false
            }
        }
    }

    async fn record_inbox(
        &self,
        message: &MessageEvent,
        outbound: Option<&str>,
        platform_id: &str,
    ) -> bool {
        self.record_inbox_with(message, outbound, platform_id, true)
            .await
    }

    async fn record_outbound(
        &self,
        message: &MessageEvent,
        outbound: Option<&str>,
        platform_id: &str,
    ) -> bool {
        self.record_inbox_with(message, outbound, platform_id, false)
            .await
    }

    async fn record_inbox_with(
        &self,
        message: &MessageEvent,
        outbound: Option<&str>,
        platform_id: &str,
        include_inbound: bool,
    ) -> bool {
        let Some(inbox) = &self.inbox else {
            return false;
        };
        let mut batch = InboxBatch::default();
        if include_inbound {
            batch.push(InboxMessage::inbound(
                message.target.id.clone(),
                message.scene,
                message.user.clone(),
                message.username.clone().unwrap_or_default(),
                message.raw_content.clone(),
                message.message_id.clone(),
            ));
        }
        if let Some(text) = outbound {
            batch.push(InboxMessage::outbound(
                message.target.id.clone(),
                message.scene,
                text,
                platform_id.to_string(),
            ));
        }
        if batch.is_empty() {
            return false;
        }
        let saved = match inbox.save(batch).await {
            Ok(()) => true,
            Err(error) => {
                self.log(
                    Level::Warn,
                    "收件箱归档失败",
                    vec![
                        ("error", error),
                        ("conversation", message.target.id.clone()),
                    ],
                );
                false
            }
        };
        if saved {
            self.publish_inbox(message, outbound);
        }
        self.refresh_conversation_name(message).await;
        saved
    }

    fn publish_inbox(&self, message: &MessageEvent, outbound: Option<&str>) {
        let Some(live) = &self.live else {
            return;
        };
        live.publish(
            aurorix_admin::TOPIC_INBOX,
            json!({
                "conv": message.target.id.clone(),
                "scene": message.scene.as_str(),
                "user": message.user.clone(),
                "user_name": message.username.clone().unwrap_or_default(),
                "content": message.raw_content.clone(),
                "reply": outbound.unwrap_or_default(),
                "platform_id": message.message_id.clone(),
                "at": aurorix_admin::live::now_ms(),
            }),
        );
    }

    fn gate_notice(&self, conversation: &str, allowed: bool) {
        enforce_gate_capacity(&self.notice_gate, self.notice_capacity);
        self.notice_gate
            .insert(conversation.to_string(), allowed);
    }

    fn remember_name_check(&self, conv_id: &str) {
        enforce_name_capacity(&self.name_refresh, self.name_capacity);
        self.name_refresh
            .insert(conv_id.to_string(), std::time::Instant::now());
    }

    async fn refresh_conversation_name(&self, message: &MessageEvent) {
        let Some(inbox) = &self.inbox else {
            return;
        };
        let conv_id = message.target.id.clone();
        if conv_id.is_empty() {
            return;
        }
        if message.scene == Scene::C2c {
            if let Some(name) = message
                .username
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                let _ = inbox.set_name(conv_id.clone(), name.to_string()).await;
                self.remember_name_check(&conv_id);
                return;
            }
        }
        let recent = match self.name_refresh.get(&conv_id) {
            Some(last) => last.elapsed() < NAME_REFRESH_TTL,
            None => false,
        };
        if recent {
            return;
        }
        self.remember_name_check(&conv_id);
        let Some(name) = self.transport.conversation_name(&message.target).await else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        if let Some(stats) = &self.stats {
            let _ = stats
                .set_name(message.scene, conv_id.clone(), name.clone())
                .await;
        }
        match inbox.set_name(conv_id.clone(), name.clone()).await {
            Ok(()) => self.log(
                Level::Debug,
                "会话名已更新",
                vec![("conversation", conv_id), ("name", name)],
            ),
            Err(error) => self.log(
                Level::Warn,
                "会话名写入失败",
                vec![("conversation", conv_id), ("error", error)],
            ),
        }
    }

    fn log(&self, level: Level, message: &str, fields: Vec<(&str, String)>) {
        let mut map = BTreeMap::new();
        for (key, value) in fields {
            if !value.is_empty() {
                map.insert(key.to_string(), value);
            }
        }
        self.logger.log(level, MODULE, message, map);
    }
}

#[derive(Debug, Clone)]
enum Piece {
    Text(String),
    Markdown(String),
    At(AtPart),
    Image(ImageSource),
    Media {
        kind: i32,
        media: Media,
    },
}

#[derive(Debug)]
enum Sender {
    Message(Outbound),
    Image {
        caption: String,
        attachment: Attachment,
    },
    ChannelImage {
        caption: String,
        url: String,
    },
    Media {
        kind: i32,
        media: Media,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Reply,
    Push,
}

impl Mode {
    const fn sent_label(self) -> &'static str {
        match self {
            Self::Reply => "回复已发送",
            Self::Push => "主动消息已发送",
        }
    }

    const fn failed_label(self) -> &'static str {
        match self {
            Self::Reply => "回复发送失败",
            Self::Push => "主动消息发送失败",
        }
    }

    const fn empty_label(self) -> &'static str {
        match self {
            Self::Reply => "回复内容为空, 不发送",
            Self::Push => "主动推送内容为空, 不发送",
        }
    }

    const fn image_label(self) -> &'static str {
        match self {
            Self::Reply => "频道图片已发送",
            Self::Push => "频道主动图片已发送",
        }
    }
}

struct EmitContext<'a> {
    target: &'a Target,
    scene: Scene,
    anchor: Anchor,
    budget: Option<Arc<SeqBudget>>,
    message_id: Option<String>,
    mode: Mode,
}

fn quote_of(reply: &Outgoing) -> Option<String> {
    reply
        .quote
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

const RULE_MIN_CHARS: usize = 3;

fn is_rule_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.chars().count() >= RULE_MIN_CHARS
        && trimmed
            .chars()
            .all(|value| value == '-' || value == '*' || value == '_')
}

fn push_markdown(inline: &mut String, markdown: &str) {
    let rule = markdown
        .lines()
        .next()
        .map(is_rule_line)
        .unwrap_or_default();
    if rule && !inline.is_empty() && !inline.ends_with("\n\n") {
        match inline.ends_with('\n') {
            true => inline.push('\n'),
            false => inline.push_str("\n\n"),
        }
    }
    inline.push_str(markdown);
}

fn has_markdown_markup(text: &str) -> bool {
    text.contains("](") || text.contains("![") || text.starts_with('#')
}

fn already_mentions(body: &str, user: &str) -> bool {
    let user = user.trim();
    if user.is_empty() {
        return true;
    }
    body.contains(&format!("<@{user}>"))
        || body.contains(&format!("<@!{user}>"))
        || body.contains(&format!("<qqbot-at-user id=\"{user}\" />"))
}

fn summary_or(summary: &str) -> &str {
    match summary.trim().is_empty() {
        true => "图片",
        false => summary,
    }
}

fn is_public_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn dims_markup(width: u32, height: u32) -> String {
    match width > 0 && height > 0 {
        true => format!(" #{width}px #{height}px"),
        false => String::new(),
    }
}

fn image_media(source: &ImageSource) -> Media {
    match source {
        ImageSource::Bytes(attachment) => {
            Media::bytes(attachment.data.clone(), attachment.filename.clone())
        }
        ImageSource::Url { url, .. } => Media::source(url.clone()),
    }
}

fn guild_image(source: ImageSource) -> Sender {
    match source {
        ImageSource::Bytes(attachment) => Sender::Image {
            caption: String::new(),
            attachment,
        },
        ImageSource::Url { url, .. } => Sender::ChannelImage {
            caption: String::new(),
            url,
        },
    }
}

async fn image_fragment(
    store: Option<Arc<dyn aurorix_api::ImageStore>>,
    target: Target,
    source: ImageSource,
) -> Result<String, String> {
    match source {
        ImageSource::Url { url, summary } if is_public_url(&url) => {
            let dims = match aurorix_assets::probe_dimensions(&url).await {
                Some((width, height)) => dims_markup(width, height),
                None => String::new(),
            };
            Ok(format!("![{}{dims}]({url})\n", summary_or(&summary)))
        }
        ImageSource::Url { url, summary } => {
            let Some(store) = store else {
                return Err(String::from("图床未配置且图片非公网 URL"));
            };
            let resolved = crate::images::with_target(target, store.ensure(&url))
                .await
                .map_err(|error| format!("图床转换失败: {error}"))?;
            Ok(format!("![{}]({resolved})\n", summary_or(&summary)))
        }
        ImageSource::Bytes(attachment) => {
            let Some(store) = store else {
                return Err(String::from("图床未配置且图片非公网 URL"));
            };
            let stored = crate::images::with_target(
                target,
                store.store(&attachment.data, attachment.filename.as_str()),
            )
            .await
            .map_err(|error| format!("图床转换失败: {error}"))?;
            Ok(format!(
                "![图片{}]({})\n",
                dims_markup(stored.width, stored.height),
                stored.url
            ))
        }
    }
}

#[derive(Debug, Clone, Default)]
struct DeliverReport {
    sent: bool,
    rendered: bool,
    archived: bool,
    error: Option<String>,
    outbound_text: Option<String>,
    platform_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushReport {
    pub sent: bool,
    pub rendered: bool,
    pub archived: bool,
    pub platform_id: String,
    pub text: Option<String>,
    pub error: Option<String>,
}

impl PushReport {
    fn from_deliver(report: DeliverReport) -> Self {
        Self {
            sent: report.sent,
            rendered: report.rendered,
            archived: report.archived,
            platform_id: report.platform_id,
            text: report.outbound_text,
            error: report.error,
        }
    }
}

struct FooterCounterInput {
    count_key: String,
    dedup: FooterDedup,
    bucket: FooterBucket,
    reply: String,
    duplicate_reply: Option<String>,
    label: String,
}

async fn mark_once(kv: &dyn KvBackend, key: &str) -> Result<bool, String> {
    if kv.get(key).await?.is_some() {
        return Ok(true);
    }
    kv.set(key, json!(true)).await?;
    Ok(false)
}

fn render_footer_template(
    template: &str,
    count: i64,
    target: &Target,
    user: &str,
    label: &str,
) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let Some(offset) = rest[start..].find("}}") else {
            break;
        };
        let end = start + offset;
        let name = rest[start + 2..end].trim();
        match name {
            "count" => out.push_str(&count.to_string()),
            "label" => out.push_str(label),
            "group" => out.push_str(&target.id),
            "user" => {
                if !user.is_empty() {
                    out.push_str(&event::mention_tag(target.scene, target.mention()));
                }
            }
            other => {
                out.push_str("{{");
                out.push_str(other);
                out.push_str("}}");
            }
        }
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

pub fn reply_kind_title(kind: ReplyKind) -> &'static str {
    match kind {
        ReplyKind::Maintenance => "维护中",
        ReplyKind::Denied => "没有权限",
        ReplyKind::Banned => "已封禁",
        ReplyKind::RateLimited => "触发太频繁",
        ReplyKind::ArgumentError => "指令参数错误",
        ReplyKind::Handler => "指令执行失败",
    }
}

fn clip(text: &str, limit: usize) -> String {
    match text.chars().count() > limit {
        true => text.chars().take(limit).collect(),
        false => text.to_string(),
    }
}

fn interaction_message(interaction: &InteractionEvent) -> MessageEvent {
    MessageEvent {
        event_type: String::from(event::INTERACTION_CREATE),
        scene: interaction.scene,
        target: interaction.target.clone(),
        user: interaction.user.clone(),
        username: None,
        role: None,
        content: String::new(),
        raw_content: String::new(),
        event_id: interaction.event_id.clone(),
        message_id: String::new(),
        mentioned: false,
        attachments: Vec::new(),
        reference: None,
    }
}

fn interaction_mentionable(interaction: &InteractionEvent) -> bool {
    interaction.scene != Scene::C2c
        && !interaction.button_id.starts_with("common.")
        && !interaction.target.id.is_empty()
        && !interaction.user.is_empty()
}

fn banned_interaction(interaction: &InteractionEvent, text: &str) -> Outgoing {
    let mut reply = Outgoing::new(ReplyKind::Banned, text).no_footer();
    reply.at = interaction_mentionable(interaction);
    reply
}

fn rate_limited_interaction(interaction: &InteractionEvent, wait: Duration) -> Outgoing {
    let mut reply = Outgoing::new(
        ReplyKind::RateLimited,
        format!("⏳ 触发太频繁了，{} 秒后再试", RateLimiter::wait_seconds(wait)),
    )
    .no_footer();
    reply.at = interaction_mentionable(interaction);
    reply
}

fn mime_of(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        "image/png"
    }
}

pub fn classify_send_error(error: &ApiError) -> (&'static str, Level) {
    match error {
        ApiError::Platform(business) => match business.kind() {
            ErrorKind::RateLimit => ("平台限流", Level::Debug),
            ErrorKind::Auth => ("平台鉴权失败", Level::Warn),
            ErrorKind::Permission => ("平台权限不足", Level::Warn),
            ErrorKind::NotFound => ("平台未找到目标", Level::Warn),
            ErrorKind::Audit => ("消息审核中", Level::Debug),
            ErrorKind::Server => ("平台服务端错误", Level::Error),
            ErrorKind::Param => ("平台参数错误", Level::Warn),
            ErrorKind::PassiveExpired => ("被动回复窗口已过期", Level::Debug),
            ErrorKind::Unknown => ("平台未分类错误", Level::Warn),
        },
        ApiError::Transport(_) => ("网络传输失败", Level::Error),
        ApiError::Status { status, .. } => match *status >= 500 {
            true => ("平台 5xx", Level::Error),
            false => ("平台 4xx", Level::Warn),
        },
        ApiError::Json(_) => ("响应解析失败", Level::Error),
        ApiError::Unsupported { .. } => ("场景不支持", Level::Warn),
        ApiError::InvalidTarget(_) => ("会话目标非法", Level::Warn),
        ApiError::ReplyBudgetExhausted { .. } => ("被动回复预算用尽", Level::Debug),
        ApiError::AuditRejected => ("消息审核未通过", Level::Warn),
        ApiError::AuditTimeout => ("消息审核超时", Level::Warn),
        ApiError::RateLimited { .. } => ("本地限流拦截", Level::Debug),
        ApiError::Upload(_) => ("媒体上传失败", Level::Warn),
        ApiError::Config(_) => ("配置错误", Level::Error),
        ApiError::Token(_) => ("令牌获取失败", Level::Error),
        ApiError::Keyboard(_) => ("键盘序列化失败", Level::Warn),
        ApiError::Fallback { last, .. } => classify_send_error(last),
    }
}

pub fn scene_label(scene: Scene) -> &'static str {
    scene.as_str()
}

fn enforce_name_capacity(cache: &DashMap<String, std::time::Instant>, capacity: usize) {
    if cache.len() >= capacity {
        cache.retain(|_, at| at.elapsed() < NAME_REFRESH_TTL);
    }
    while cache.len() >= capacity {
        let victim = cache.iter().next().map(|entry| entry.key().clone());
        let Some(key) = victim else {
            break;
        };
        cache.remove(&key);
    }
}

fn enforce_gate_capacity(gate: &DashMap<String, bool>, capacity: usize) {
    while gate.len() >= capacity {
        let victim = gate.iter().next().map(|entry| entry.key().clone());
        let Some(key) = victim else {
            break;
        };
        gate.remove(&key);
    }
}

fn conversation_key(scene: Scene, target: &Target) -> String {
    format!("{}:{}", scene.as_str(), target.id)
}

fn trim_cache<V>(cache: &DashMap<String, V>, capacity: usize) {
    while cache.len() >= capacity {
        let victim = cache.iter().next().map(|entry| entry.key().clone());
        let Some(key) = victim else {
            break;
        };
        cache.remove(&key);
    }
}

fn push_sink_of(weak: std::sync::Weak<Pipeline>) -> aurorix_plugin::PushSink {
    aurorix_plugin::PushSink::new(move |target: Target, outgoing: Outgoing| {
        let weak = weak.clone();
        Box::pin(async move {
            let Some(pipeline) = weak.upgrade() else {
                return Err(aurorix_plugin::PluginError::reply(String::from(
                    "框架已停止, 主动推送没有发出去",
                )));
            };
            let scene = target.scene;
            let report = pipeline.push(&target, scene, &outgoing).await;
            match report.sent {
                true => Ok(()),
                false => Err(aurorix_plugin::PluginError::reply(
                    report
                        .error
                        .unwrap_or_else(|| String::from("主动推送失败")),
                )),
            }
        })
    })
}

fn enforce_budget_capacity(cache: &DashMap<String, Arc<SeqBudget>>, capacity: usize) {
    while cache.len() >= capacity {
        let victim = cache.iter().next().map(|entry| entry.key().clone());
        let Some(key) = victim else {
            break;
        };
        cache.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_cache_stays_within_capacity() {
        let cache: DashMap<String, std::time::Instant> = DashMap::new();
        for index in 0..5000 {
            enforce_name_capacity(&cache, 64);
            cache.insert(format!("conv-{index}"), std::time::Instant::now());
        }
        assert!(
            cache.len() <= 64,
            "会话名缓存超过上限, 实际 {} 条",
            cache.len()
        );
    }

    #[test]
    fn name_cache_with_tiny_capacity_terminates() {
        let cache: DashMap<String, std::time::Instant> = DashMap::new();
        for index in 0..200 {
            enforce_name_capacity(&cache, 1);
            cache.insert(format!("conv-{index}"), std::time::Instant::now());
        }
        assert_eq!(cache.len(), 1, "容量为 1 时只该留一条");
    }

    struct NoticeTransport {
        sent: std::sync::Mutex<Vec<Outbound>>,
    }

    impl ReplyTransport for NoticeTransport {
        fn send<'a>(
            &'a self,
            target: &'a Target,
            message: &'a Outbound,
        ) -> aurorix_gateway::BoxFuture<'a, Result<aurorix_api::SendOutcome, ApiError>> {
            let target = target.clone();
            let message = message.clone();
            Box::pin(async move {
                self.sent.lock().expect("锁可用").push(message.clone());
                Ok(aurorix_api::SendOutcome {
                    message_id: Some(String::from("out-1")),
                    audit_id: None,
                    target,
                    delivered_active: message.initiative,
                    fallback: false,
                })
            })
        }
    }

    fn member_add_event() -> ProtocolEvent {
        ProtocolEvent {
            id: Some(String::from("evt-welcome")),
            op: 0,
            event_type: Some(String::from("GROUP_MEMBER_ADD")),
            seq: Some(1),
            data: json!({
                "group_openid": "g1",
                "member_openid": "u1",
                "timestamp": 1_700_000_000
            }),
            raw: String::new(),
        }
    }

    #[tokio::test]
    async fn welcome_keeps_the_blue_command_tag_clickable() {
        let transport = Arc::new(NoticeTransport {
            sent: std::sync::Mutex::new(Vec::new()),
        });
        let mut config = BotConfig::new("app", "secret");
        config.welcome_messages = Some(vec![String::from("欢迎加入, 点下面的蓝色文字试试")]);
        config.welcome_extra =
            Some(String::from("> <qqbot-cmd-input text=\"/a\" show=\"arc菜单\" />"));
        let logger = Logger::new(aurorix_log::Options::default().with_console(false));
        let pipeline = Pipeline::new(PipelineDeps::new(
            Arc::new(config),
            Arc::new(Router::new(Arc::new(
                aurorix_plugin::CommandRegistry::new(),
            ))),
            Arc::clone(&transport) as Arc<dyn ReplyTransport>,
            logger,
        ));
        let report = pipeline.handle(&member_add_event()).await;
        assert!(report.sent, "入群欢迎要发出去: {report:?}");
        let sent = transport.sent.lock().expect("锁可用");
        assert_eq!(
            sent[0].kind,
            aurorix_protocol::MessageKind::Markdown,
            "入群欢迎必须走 markdown, 否则蓝色点击文字会退化成纯文本"
        );
        assert!(
            sent[0].content.contains("<qqbot-cmd-input"),
            "附加内容里的指令标签不能被摊平成纯文字: {}",
            sent[0].content
        );
        assert!(
            sent[0].content.contains("%2Fa"),
            "标签里的指令要由框架代劳 urlencode: {}",
            sent[0].content
        );
    }

    #[test]
    fn markdown_hint_keeps_a_blank_line_after_the_body() {
        let mut inline = String::from("正文");
        push_markdown(&mut inline, "---\n\n> **🔗补齐别名**");
        assert_eq!(inline, "正文\n\n---\n\n> **🔗补齐别名**");
        let mut spaced = String::from("正文\n\n");
        push_markdown(&mut spaced, "---\n\n> **🔗补齐别名**");
        assert_eq!(spaced, "正文\n\n---\n\n> **🔗补齐别名**");
        let mut empty = String::new();
        push_markdown(&mut empty, "---\n\n> **🔗补齐别名**");
        assert_eq!(empty, "---\n\n> **🔗补齐别名**");
    }

    #[test]
    fn plain_markdown_pieces_are_not_padded() {
        let mut inline = String::from("成绩");
        push_markdown(&mut inline, "![图片](https://e/a.png)");
        assert_eq!(inline, "成绩![图片](https://e/a.png)");
    }

    #[test]
    fn text_with_markdown_links_is_sent_as_markdown() {
        assert!(has_markdown_markup("欢迎 [点这里](https://e/a) 查看"));
        assert!(has_markdown_markup("![图](https://e/a.png)"));
        assert!(has_markdown_markup("### 帮助"));
        assert!(!has_markdown_markup("指令执行失败: 假处理器炸了"));
        assert!(!has_markdown_markup("纯文字回复"));
    }

    #[test]
    fn same_user_is_mentioned_once_per_message() {
        let transport = Arc::new(NoticeTransport {
            sent: std::sync::Mutex::new(Vec::new()),
        });
        let logger = Logger::new(aurorix_log::Options::default().with_console(false));
        let pipeline = Pipeline::new(PipelineDeps::new(
            Arc::new(BotConfig::new("app", "secret")),
            Arc::new(Router::new(Arc::new(
                aurorix_plugin::CommandRegistry::new(),
            ))),
            Arc::clone(&transport) as Arc<dyn ReplyTransport>,
            logger,
        ));
        let target = Target::new(aurorix_core::Scene::Group, "g1").with_user("u1");
        let mut reply = Outgoing::new(ReplyKind::Handler, "正文");
        reply.at = true;
        reply.mention_user = Some(String::from("u1"));
        let prefix = pipeline.mention_prefix(aurorix_core::Scene::Group, &target, &reply, false);
        assert_eq!(
            prefix.matches("<@u1>").count(),
            1,
            "同一条消息里同一个用户只能被 @ 一次: {prefix}"
        );
    }

}
