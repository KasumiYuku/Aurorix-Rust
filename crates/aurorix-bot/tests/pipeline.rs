use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_api::{ApiError, Outbound, SendOutcome};
use aurorix_bot::{
    Bot, BotConfig, ClosurePluginConfig, ControlCommand, EventCategory, Frame, FrameIo,
    GatewayState, InboxArchive, InboxBatch, KvBackend, Logger, MemoryInbox, MemoryStats, Outcome,
    Pipeline, PluginConfigurable, Rendered, Renderer, ReplyTransport, StatsDelta, StatsRecorder,
    TokenSource,
};
use aurorix_core::Target;
use aurorix_gateway::{
    BoxFuture, ConnectRequest, GatewayError, ProtocolEvent, StaticTokenSource, Transport,
};
use aurorix_log::{Level, Options};
use aurorix_plugin::{
    CommandRegistry, CommandSpec, Invocation, Outgoing, PluginCommands, PluginError, ReplyKind,
};
use aurorix_render::RenderError;
use serde_json::json;

const EVENT_WAIT: Duration = Duration::from_secs(3);

type ImageSend = (String, String, usize, Option<String>);

struct FakeTransport {
    sent: Mutex<Vec<(Target, String)>>,
    media: Mutex<Vec<Outbound>>,
    images: Mutex<Vec<ImageSend>>,
    captions: Mutex<Vec<String>>,
    uploads: Mutex<Vec<String>>,
    media_uploads: Mutex<Vec<i32>>,
    names: Mutex<Vec<String>>,
    name_calls: AtomicUsize,
    failures: AtomicUsize,
    status: u16,
}

impl FakeTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
            media: Mutex::new(Vec::new()),
            images: Mutex::new(Vec::new()),
            captions: Mutex::new(Vec::new()),
            uploads: Mutex::new(Vec::new()),
            media_uploads: Mutex::new(Vec::new()),
            names: Mutex::new(Vec::new()),
            name_calls: AtomicUsize::new(0),
            failures: AtomicUsize::new(0),
            status: 500,
        })
    }

    fn contents(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|(_, content)| content.clone())
            .collect()
    }

    fn count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }

    fn fail_next(&self, times: usize) {
        self.failures.store(times, Ordering::SeqCst);
    }

    fn media(&self) -> Vec<Outbound> {
        self.media.lock().unwrap().clone()
    }

    fn images(&self) -> Vec<(String, String, usize, Option<String>)> {
        self.images.lock().unwrap().clone()
    }

    fn captions(&self) -> Vec<String> {
        self.captions.lock().unwrap().clone()
    }

    fn uploads(&self) -> Vec<String> {
        self.uploads.lock().unwrap().clone()
    }

    fn media_uploads(&self) -> Vec<i32> {
        self.media_uploads.lock().unwrap().clone()
    }

    fn provide_name(&self, name: &str) {
        self.names.lock().unwrap().push(name.to_string());
    }

    fn name_calls(&self) -> usize {
        self.name_calls.load(Ordering::SeqCst)
    }
}

impl ReplyTransport for FakeTransport {
    fn send<'a>(
        &'a self,
        target: &'a Target,
        message: &'a Outbound,
    ) -> BoxFuture<'a, Result<SendOutcome, ApiError>> {
        let target = target.clone();
        let message = message.clone();
        Box::pin(async move {
            if self.failures.load(Ordering::SeqCst) > 0 {
                self.failures.fetch_sub(1, Ordering::SeqCst);
                return Err(ApiError::Status {
                    status: self.status,
                    body: String::from("假发送通道故障"),
                });
            }
            let index = self.count() + 1;
            self.sent
                .lock()
                .unwrap()
                .push((target.clone(), message.content.clone()));
            self.media.lock().unwrap().push(message.clone());
            Ok(SendOutcome {
                message_id: Some(format!("out-{index}")),
                audit_id: None,
                target,
                delivered_active: message.initiative,
                fallback: false,
            })
        })
    }

    fn conversation_name<'a>(&'a self, _target: &'a Target) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move {
            self.name_calls.fetch_add(1, Ordering::SeqCst);
            self.names.lock().unwrap().first().cloned()
        })
    }

    fn upload_image<'a>(
        &'a self,
        _target: &'a Target,
        data: Vec<u8>,
        filename: String,
    ) -> BoxFuture<'a, Result<aurorix_api::MediaResult, ApiError>> {
        Box::pin(async move {
            self.uploads
                .lock()
                .unwrap()
                .push(format!("{filename}:{}", data.len()));
            Ok(aurorix_api::MediaResult {
                file_info: format!("fi-{filename}-{}", data.len()),
                file_uuid: String::from("uuid-1"),
                ttl: 0,
                raw_url: String::new(),
            })
        })
    }

    fn send_image_bytes<'a>(
        &'a self,
        _target: &'a Target,
        content: String,
        data: Vec<u8>,
        filename: String,
        mime_type: String,
        msg_id: Option<String>,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        Box::pin(async move {
            self.captions.lock().unwrap().push(content);
            self.images
                .lock()
                .unwrap()
                .push((filename, mime_type, data.len(), msg_id));
            Ok(Some(String::from("img-1")))
        })
    }

    fn upload_media<'a>(
        &'a self,
        _target: &'a Target,
        upload: &'a aurorix_api::MediaUpload,
    ) -> BoxFuture<'a, Result<aurorix_api::MediaResult, ApiError>> {
        Box::pin(async move {
            self.media_uploads.lock().unwrap().push(upload.file_type);
            Ok(aurorix_api::MediaResult {
                file_info: format!("media-{}", upload.file_type),
                file_uuid: String::from("uuid-media"),
                ttl: 0,
                raw_url: String::new(),
            })
        })
    }
}

struct FakeRenderer {
    fail: bool,
    calls: Arc<AtomicUsize>,
}

impl FakeRenderer {
    fn new(fail: bool) -> (Arc<Self>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(Self {
                fail,
                calls: Arc::clone(&calls),
            }),
            calls,
        )
    }
}

impl Renderer for FakeRenderer {
    fn render<'a>(
        &'a self,
        card: &'a aurorix_bot::CardView,
    ) -> BoxFuture<'a, Result<Rendered, RenderError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let fail = self.fail;
        let text = card.body.clone();
        Box::pin(async move {
            if fail {
                return Err(RenderError::Backend(String::from("假渲染器故障")));
            }
            Ok(Rendered::Text(format!("【卡片】{text}")))
        })
    }

    fn render_card<'a>(
        &'a self,
        card: &'a aurorix_plugin::RenderCard,
    ) -> BoxFuture<'a, Result<Rendered, RenderError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let fail = self.fail;
        let label = card.label.clone();
        Box::pin(async move {
            if fail {
                return Err(RenderError::Backend(String::from("假渲染器故障")));
            }
            Ok(Rendered::Text(format!("【卡片】{label}")))
        })
    }
}

struct FakeIo {
    frames: Mutex<VecDeque<Frame>>,
    closed: AtomicBool,
    sent: Mutex<Vec<Frame>>,
}

impl FakeIo {
    fn new(frames: Vec<Frame>) -> Arc<Self> {
        Arc::new(Self {
            frames: Mutex::new(frames.into_iter().collect()),
            closed: AtomicBool::new(false),
            sent: Mutex::new(Vec::new()),
        })
    }

    fn push(&self, frame: Frame) {
        self.frames.lock().unwrap().push_back(frame);
    }
}

impl FrameIo for FakeIo {
    fn recv(&self) -> BoxFuture<'_, Result<Frame, GatewayError>> {
        Box::pin(async move {
            loop {
                if let Some(frame) = self.frames.lock().unwrap().pop_front() {
                    return Ok(frame);
                }
                if self.closed.load(Ordering::SeqCst) {
                    return Ok(Frame::Close(1000));
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
    }

    fn send(&self, frame: Frame) -> BoxFuture<'_, Result<(), GatewayError>> {
        self.sent.lock().unwrap().push(frame);
        Box::pin(async move { Ok(()) })
    }

    fn close(&self, _code: u16) -> BoxFuture<'_, Result<(), GatewayError>> {
        self.closed.store(true, Ordering::SeqCst);
        Box::pin(async move { Ok(()) })
    }
}

struct FakeGateway {
    ios: Mutex<Vec<Arc<FakeIo>>>,
}

impl FakeGateway {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            ios: Mutex::new(Vec::new()),
        })
    }

    fn push_text(&self, text: &str) {
        for io in self.ios.lock().unwrap().iter() {
            io.push(Frame::Text(text.to_string()));
        }
    }

    fn connections(&self) -> usize {
        self.ios.lock().unwrap().len()
    }
}

impl Transport for FakeGateway {
    fn connect<'a>(
        &'a self,
        _request: ConnectRequest,
    ) -> BoxFuture<'a, Result<Arc<dyn FrameIo>, GatewayError>> {
        Box::pin(async move {
            let io = FakeIo::new(vec![
                Frame::Text(String::from(
                    r#"{"op":10,"d":{"heartbeat_interval":600000}}"#,
                )),
                Frame::Text(String::from(
                    r#"{"op":0,"s":1,"t":"READY","d":{"session_id":"s1","user":{"id":"bot1","username":"测试机器人"}}}"#,
                )),
            ]);
            self.ios.lock().unwrap().push(Arc::clone(&io));
            Ok(io as Arc<dyn FrameIo>)
        })
    }
}

fn test_logger() -> Arc<Logger> {
    Logger::new(
        Options::default()
            .with_console(false)
            .with_level(Level::Debug),
    )
}

fn test_config() -> BotConfig {
    BotConfig::new("appid-test", "secret-test")
        .with_intents(vec![String::from("GROUP_AT_MESSAGE_CREATE")])
        .with_admin("127.0.0.1", 18080)
}

fn registry_with(calls: Arc<Mutex<Vec<String>>>) -> Arc<CommandRegistry> {
    let registry = CommandRegistry::new();
    registry.register_plugin(
        PluginCommands::new("demo", "示例插件")
            .describe("测试用插件")
            .command(CommandSpec::new("echo").describe("复读").on_sync(
                move |invocation: &Invocation| {
                    calls.lock().unwrap().push(invocation.text.clone());
                    Ok(Some(Outgoing::new(
                        ReplyKind::Handler,
                        format!("echo: {}", invocation.text),
                    )))
                },
            ))
            .command(CommandSpec::new("card").describe("卡片图").on_sync(
                |_invocation: &Invocation| {
                    let keyboard = aurorix_buttons::Builder::new()
                        .scope("demo")
                        .callback(
                            "again",
                            "再来一张",
                            "again",
                            vec![aurorix_buttons::visited("已点")],
                        )
                        .build()
                        .ok_or_else(|| PluginError::handler("键盘构建失败"))?;
                    Ok(Some(
                        Outgoing::new(ReplyKind::Handler, "成绩卡")
                            .image(aurorix_plugin::Attachment::png(vec![
                                0x89, 0x50, 0x4e, 0x47,
                            ]))
                            .keyboard(keyboard),
                    ))
                },
            ))
            .command(CommandSpec::new("empty_card").describe("空图").on_sync(
                |_invocation: &Invocation| {
                    Ok(Some(
                        Outgoing::new(ReplyKind::Handler, "只有文字")
                            .image(aurorix_plugin::Attachment::png(Vec::new())),
                    ))
                },
            ))
            .command(CommandSpec::new("card_page").describe("渲染卡片").on_sync(
                |_invocation: &Invocation| {
                    let card = aurorix_plugin::RenderCard::new(
                        aurorix_render::RenderRequest::new(64, 64),
                        "页面.png",
                        |_kit| {},
                    );
                    Ok(Some(Outgoing::new(ReplyKind::Handler, "文字页").card(card)))
                },
            ))
            .command(CommandSpec::new("progress").describe("分步回复").on(
                |invocation: Invocation| async move {
                    invocation
                        .send(Outgoing::new(ReplyKind::Handler, "正在查询…"))
                        .await?;
                    Ok(Some(Outgoing::new(ReplyKind::Handler, "查询完成")))
                },
            ))
            .command(
                CommandSpec::new("boom")
                    .describe("必定失败")
                    .on_sync(|_invocation: &Invocation| Err(PluginError::handler("假处理器炸了"))),
            ),
    );
    Arc::new(registry)
}

struct Harness {
    bot: Bot,
    transport: Arc<FakeTransport>,
    stats: Arc<MemoryStats>,
    inbox: Arc<MemoryInbox>,
    calls: Arc<Mutex<Vec<String>>>,
    logger: Arc<Logger>,
}

impl Harness {
    fn pipeline(&self) -> &Arc<Pipeline> {
        self.bot.pipeline()
    }

    fn build(config: BotConfig) -> Self {
        let transport = FakeTransport::new();
        let stats = Arc::new(MemoryStats::new());
        let inbox = Arc::new(MemoryInbox::new());
        let kv = Arc::new(aurorix_bot::MemoryKv::new());
        let logger = test_logger();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let bot = Bot::builder(config)
            .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
            .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
            .with_plugins(registry_with(Arc::clone(&calls)))
            .with_stats(Arc::clone(&stats) as Arc<dyn StatsRecorder>)
            .with_inbox(Arc::clone(&inbox) as Arc<dyn InboxArchive>)
            .with_kv(Arc::clone(&kv) as Arc<dyn KvBackend>)
            .with_logger(Arc::clone(&logger))
            .with_signal_handling(false)
            .with_instance_lock(false)
            .build()
            .expect("测试机器人要能装配");
        Self {
            bot,
            transport,
            stats,
            inbox,
            calls,
            logger,
        }
    }
}

fn group_event(event_id: &str, message_id: &str, content: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(event_id.to_string()),
        op: 0,
        event_type: Some(String::from("GROUP_AT_MESSAGE_CREATE")),
        seq: Some(2),
        data: json!({
            "id": message_id,
            "content": content,
            "group_openid": "g1",
            "author": {
                "member_openid": "u1",
                "username": "小明",
                "member_role": "member"
            }
        }),
        raw: String::new(),
    }
}

fn event_frame(event_id: &str, message_id: &str, content: &str) -> String {
    json!({
        "op": 0,
        "id": event_id,
        "s": 3,
        "t": "GROUP_AT_MESSAGE_CREATE",
        "d": {
            "id": message_id,
            "content": content,
            "group_openid": "g1",
            "author": {
                "member_openid": "u1",
                "username": "小明",
                "member_role": "member"
            }
        }
    })
    .to_string()
}

async fn wait_until(check: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + EVENT_WAIT;
    while tokio::time::Instant::now() < deadline {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("条件在 {EVENT_WAIT:?} 内没有满足");
}

fn log_contains(logger: &Logger, needle: &str) -> bool {
    logger
        .recent(200)
        .iter()
        .any(|record| record.msg.contains(needle))
}

#[tokio::test]
async fn group_message_dispatches_command_and_records_stats_and_inbox() {
    let harness = Harness::build(test_config());
    let report = harness
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/echo hi"))
        .await;
    assert_eq!(report.outcome, Outcome::Replied);
    assert_eq!(report.category, EventCategory::Message);
    assert!(report.sent, "回复必须发出去");
    assert!(report.stats_recorded, "统计必须写一次");
    assert!(report.inbox_recorded, "收件箱必须归档一次");
    assert_eq!(harness.calls.lock().unwrap().as_slice(), ["hi".to_string()]);
    assert_eq!(harness.transport.contents(), vec!["echo: hi".to_string()]);
    assert_eq!(harness.stats.calls(), 1, "stats 每条只写一次");
    assert_eq!(harness.stats.total_received(), 1);
    assert_eq!(harness.stats.total_sent(), 1);
    let batches = harness.inbox.saved();
    assert_eq!(batches.len(), 1, "inbox 每条只归档一次");
    assert_eq!(batches[0].len(), 2, "入站与出站各一条");
    assert_eq!(batches[0].messages[0].platform_id, "msg-1");
}

#[tokio::test]
async fn duplicate_event_id_is_dropped() {
    let harness = Harness::build(test_config());
    let first = harness
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/echo hi"))
        .await;
    let second = harness
        .pipeline()
        .handle(&group_event("evt-1", "msg-2", "/echo hi"))
        .await;
    assert_eq!(first.outcome, Outcome::Replied);
    assert_eq!(second.outcome, Outcome::Duplicate);
    assert!(!second.sent);
    assert_eq!(harness.transport.count(), 1, "重复 event_id 只允许回一次");
    assert_eq!(harness.stats.calls(), 1, "重复事件不写统计");
    assert_eq!(harness.pipeline().counters().duplicates, 1);
}

#[tokio::test]
async fn duplicate_falls_back_to_message_id_when_event_id_missing() {
    let harness = Harness::build(test_config());
    let mut event = group_event("ignored", "msg-9", "/echo hi");
    event.id = None;
    let first = harness.pipeline().handle(&event).await;
    let second = harness.pipeline().handle(&event).await;
    assert_eq!(first.outcome, Outcome::Replied);
    assert_eq!(second.outcome, Outcome::Duplicate);
    assert_eq!(harness.transport.count(), 1);
}

#[tokio::test]
async fn maintenance_blocks_with_fixed_text() {
    let config = test_config().with_maintenance(Some(String::from("维护中, 请稍后再试")));
    let harness = Harness::build(config);
    let report = harness
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/echo hi"))
        .await;
    assert_eq!(
        report.outcome,
        Outcome::Replied,
        "维护话术由路由层按指令给出, 框架只负责发出去"
    );
    assert!(report.sent);
    assert!(
        harness.calls.lock().unwrap().is_empty(),
        "维护模式下不该执行指令"
    );
    let chatter = harness
        .pipeline()
        .handle(&group_event("evt-2", "msg-2", "今天吃什么"))
        .await;
    assert!(!chatter.sent, "闲聊不是指令, 不该被维护话术刷屏");
    let sent = harness.transport.contents();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("维护中, 请稍后再试"));
    assert!(sent[0].contains("<@u1>"), "群聊维护提示要艾特发起人");
    assert_eq!(harness.stats.calls(), 1, "only the command that hit counts");
}

#[tokio::test]
async fn unknown_command_is_silent() {
    let harness = Harness::build(test_config());
    let report = harness
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/nope hi"))
        .await;
    assert_eq!(report.outcome, Outcome::NotCommand);
    assert!(report.reply_text.is_none(), "未命中指令不回复");
    assert_eq!(harness.transport.count(), 0);
    assert!(harness.calls.lock().unwrap().is_empty());
    assert!(report.stats_recorded, "未命中指令仍然要计收到");
    assert_eq!(harness.stats.total_received(), 1);
    assert_eq!(harness.stats.total_sent(), 0);
}

#[tokio::test]
async fn handler_error_replies_and_next_event_still_works() {
    let harness = Harness::build(test_config());
    let failed = harness
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/boom"))
        .await;
    assert_eq!(failed.outcome, Outcome::Replied);
    let text = failed.reply_text.clone().expect("错误也要回话");
    assert!(text.contains("指令执行失败"), "错误文案要带标题: {text}");
    assert!(text.contains("假处理器炸了"), "错误文案要带原因: {text}");
    let ok = harness
        .pipeline()
        .handle(&group_event("evt-2", "msg-2", "/echo next"))
        .await;
    assert_eq!(ok.outcome, Outcome::Replied);
    assert_eq!(
        harness.transport.contents(),
        vec![format!("<@u1>{text}"), String::from("echo: next")]
    );
}

#[tokio::test]
async fn send_failure_does_not_panic_and_is_logged() {
    let harness = Harness::build(test_config());
    harness.transport.fail_next(1);
    let failed = harness
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/echo hi"))
        .await;
    assert_eq!(failed.outcome, Outcome::SendFailed);
    assert!(!failed.sent);
    let error = failed.send_error.clone().expect("失败原因要留下");
    assert!(error.contains("500"), "要带上平台状态码: {error}");
    assert!(
        log_contains(&harness.logger, "回复发送失败"),
        "发送失败必须记日志"
    );
    assert_eq!(harness.pipeline().counters().send_failed, 1);
    let ok = harness
        .pipeline()
        .handle(&group_event("evt-2", "msg-2", "/echo again"))
        .await;
    assert_eq!(ok.outcome, Outcome::Replied, "失败不能影响下一条");
    assert_eq!(
        harness.transport.contents(),
        vec![String::from("echo: again")]
    );
    assert_eq!(harness.stats.total_sent(), 1, "只有成功的才记发出");
}

#[tokio::test]
async fn plain_text_replies_are_never_rendered() {
    let transport = FakeTransport::new();
    let (renderer, calls) = FakeRenderer::new(false);
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_render(renderer as Arc<dyn Renderer>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let report = bot
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/boom"))
        .await;
    assert_eq!(calls.load(Ordering::SeqCst), 0, "纯文本回复不许转成图片");
    assert!(!report.rendered, "没有卡片就不该标记已渲染");
    let sent = transport.contents();
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].contains("指令执行失败"),
        "错误文案要保持纯文本: {}",
        sent[0]
    );
    let _ = bot.shutdown(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn card_render_success_replaces_the_card_slot() {
    let transport = FakeTransport::new();
    let (renderer, calls) = FakeRenderer::new(false);
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_render(renderer as Arc<dyn Renderer>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let report = bot
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/card_page"))
        .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1, "插件显式卡片要渲染");
    assert!(report.rendered);
    let sent = transport.contents();
    assert!(
        sent[0].contains("【卡片】页面.png"),
        "渲染结果要替换卡片位: {}",
        sent[0]
    );
    let _ = bot.shutdown(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn broken_plugin_config_does_not_break_other_plugins() {
    let good = Arc::new(ClosurePluginConfig::new("demo", |settings| {
        assert!(settings.is_empty(), "没有配置时是空表");
        Ok(())
    }));
    let bad = Arc::new(ClosurePluginConfig::new("broken", |_settings| {
        Err(String::from("字段 type 不合法"))
    }));
    let transport = FakeTransport::new();
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .with_plugin_config(good as Arc<dyn PluginConfigurable>)
        .with_plugin_config(bad as Arc<dyn PluginConfigurable>)
        .build()
        .expect("装配失败");
    let handle = bot.start().await.expect("启动失败");
    let failures = bot.config_failures();
    assert_eq!(failures.len(), 1, "只有一个插件配置坏掉: {failures:?}");
    assert!(failures[0].contains("broken"));
    assert!(failures[0].contains("退回默认值"), "错误要可诊断");
    let report = bot
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/echo hi"))
        .await;
    assert_eq!(report.outcome, Outcome::Replied, "坏配置不该带走整台机器人");
    assert!(handle.live_tasks() > 0, "启动后应当有后台任务");
    let shutdown = bot
        .shutdown(Duration::from_secs(3))
        .await
        .expect("停机失败");
    assert_eq!(shutdown.live_tasks, 0);
}

#[tokio::test]
async fn shutdown_leaves_no_live_tasks_and_stops_gateway() {
    let gateway = FakeGateway::new();
    let transport = FakeTransport::new();
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_gateway_transport(Arc::clone(&gateway) as Arc<dyn Transport>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let handle = bot.start().await.expect("启动失败");
    wait_until(|| gateway.connections() == 1).await;
    wait_until(|| handle.gateway_state() == GatewayState::Ready).await;
    assert!(handle.live_tasks() >= 1, "网关与事件循环都要计入后台任务");
    let report = bot
        .shutdown(Duration::from_secs(3))
        .await
        .expect("停机失败");
    assert_eq!(report.live_tasks, 0, "停机后不允许留下后台任务");
    assert_eq!(bot.live_tasks(), 0, "live_tasks() 要能断言为 0");
    assert_eq!(bot.gateway_state(), GatewayState::Stopped, "网关必须先停");
    assert!(report.gateway_state == GatewayState::Stopped);
    assert!(!report.timed_out);
}

#[tokio::test]
async fn events_keep_order_for_the_same_user() {
    let gateway = FakeGateway::new();
    let transport = FakeTransport::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_gateway_transport(Arc::clone(&gateway) as Arc<dyn Transport>)
        .with_plugins(registry_with(Arc::clone(&calls)))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let handle = bot.start().await.expect("启动失败");
    wait_until(|| handle.gateway_state() == GatewayState::Ready).await;
    for index in 1..=3 {
        gateway.push_text(&event_frame(
            &format!("evt-{index}"),
            &format!("msg-{index}"),
            &format!("/echo {index}"),
        ));
    }
    wait_until(|| transport.count() == 3).await;
    assert_eq!(
        transport.contents(),
        vec![
            String::from("echo: 1"),
            String::from("echo: 2"),
            String::from("echo: 3"),
        ],
        "同一用户的连续消息顺序不能乱"
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["1".to_string(), "2".to_string(), "3".to_string()]
    );
    let report = bot
        .shutdown(Duration::from_secs(3))
        .await
        .expect("停机失败");
    assert_eq!(report.live_tasks, 0);
}

#[tokio::test]
async fn control_stop_finishes_the_event_loop() {
    let gateway = FakeGateway::new();
    let bot = Bot::builder(test_config())
        .with_transport(FakeTransport::new() as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_gateway_transport(Arc::clone(&gateway) as Arc<dyn Transport>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let handle = bot.start().await.expect("启动失败");
    wait_until(|| handle.gateway_state() == GatewayState::Ready).await;
    assert!(handle.request_stop(), "控制通道要能收下 stop");
    let command = tokio::time::timeout(EVENT_WAIT, handle.wait())
        .await
        .expect("事件循环必须被 stop 结束");
    assert_eq!(command, ControlCommand::Stop);
    assert!(handle.is_finished());
    let report = bot
        .shutdown(Duration::from_secs(3))
        .await
        .expect("停机失败");
    assert_eq!(report.control, ControlCommand::Stop);
    assert_eq!(report.live_tasks, 0);
}

#[tokio::test]
async fn serve_runs_until_stop_and_returns_report() {
    let gateway = FakeGateway::new();
    let transport = FakeTransport::new();
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_gateway_transport(Arc::clone(&gateway) as Arc<dyn Transport>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let handle = bot.handle();
    let serving = tokio::spawn(async move { bot.serve().await });
    wait_until(|| handle.gateway_state() == GatewayState::Ready).await;
    gateway.push_text(&event_frame("evt-1", "msg-1", "/echo serve"));
    wait_until(|| transport.count() == 1).await;
    assert!(handle.request_restart());
    let report = tokio::time::timeout(Duration::from_secs(5), serving)
        .await
        .expect("serve 必须能退出")
        .expect("serve 任务不该 panic")
        .expect("serve 不该报错");
    assert_eq!(report.control, ControlCommand::Restart);
    assert_eq!(report.live_tasks, 0);
    assert_eq!(report.gateway_state, GatewayState::Stopped);
}

#[tokio::test]
async fn missing_dependency_reports_chinese_error() {
    let error = Bot::builder(test_config())
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .build()
        .expect_err("缺发送通道必须报错");
    let text = error.to_string();
    assert!(text.contains("ReplyTransport"), "错误要指出缺什么: {text}");
    assert!(text.contains("发送通道未注入"), "错误要说明后果: {text}");
    assert!(text.contains("with_*"), "错误要给出修复方式: {text}");

    let error = Bot::builder(test_config())
        .with_transport(FakeTransport::new() as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .build()
        .expect_err("缺统计必须报错");
    let text = error.to_string();
    assert!(text.contains("StatsRecorder"), "{text}");
    assert!(text.contains("统计没有落点"), "{text}");
}

#[tokio::test]
async fn fallback_policy_supplies_memory_implementations() {
    let bot = Bot::builder(test_config())
        .with_transport(FakeTransport::new() as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .allow_fallbacks()
        .build()
        .expect("允许兜底时应当能装配");
    assert!(bot.pipeline().has_kv(), "内存 KV 要顶上");
    assert!(bot.pipeline().has_stats(), "内存统计要顶上");
    assert!(!bot.pipeline().has_inbox(), "没注入就没有收件箱");
    assert!(!bot.pipeline().has_renderer(), "没注入就没有渲染器");
    let report = bot
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/echo hi"))
        .await;
    assert_eq!(report.outcome, Outcome::Replied);
    assert_eq!(
        bot.pipeline().counters().received,
        1,
        "内存统计也要累计计数"
    );
}

#[tokio::test]
async fn config_from_app_config_carries_required_fields() {
    let mut app = aurorix_db::AppConfig {
        appid: String::from("app-1"),
        secret: String::from("secret-1"),
        port: 9000,
        admin_password: String::from("pwd"),
        protocol: Some(String::from("websocket")),
        proxy: String::from("https://example.invalid"),
        database: String::from("postgres://u@127.0.0.1:5432/db"),
        log_level: Some(String::from("debug")),
        gateway_url: Some(String::from("wss://example.invalid/ws")),
        intents: Some(vec![String::from("AT_MESSAGE_CREATE")]),
        ..aurorix_db::AppConfig::default()
    };
    app.normalize();
    let config = BotConfig::from_app_config(&app);
    assert_eq!(config.appid, "app-1");
    assert_eq!(config.secret, "secret-1");
    assert_eq!(config.admin_port, 9000);
    assert_eq!(config.admin_password, "pwd");
    assert_eq!(config.proxy_api, "https://example.invalid");
    assert_eq!(config.database, "postgres://u@127.0.0.1:5432/db");
    assert_eq!(config.log_level, "debug");
    assert_eq!(
        config.gateway_url.as_deref(),
        Some("wss://example.invalid/ws")
    );
    assert_eq!(
        config.render_pool,
        BotConfig::adaptive_render_pool(),
        "没写 render_pool 时按机器档位自适应"
    );
    assert!(config.validate().is_ok());
    assert_eq!(
        config.intent_bits(),
        aurorix_gateway::bits_for_names(&config.intents)
    );
    assert!(config.needs_websocket());
}

#[tokio::test]
async fn channel_message_without_mention_is_ignored() {
    let harness = Harness::build(test_config());
    let mut event = group_event("evt-1", "msg-1", "/echo hi");
    event.event_type = Some(String::from("MESSAGE_CREATE"));
    event.data = json!({
        "id": "msg-1",
        "content": "/echo hi",
        "channel_id": "c1",
        "author": { "id": "42", "username": "频道用户" },
        "mentions": []
    });
    let report = harness.pipeline().handle(&event).await;
    assert_eq!(report.outcome, Outcome::Ignored);
    assert_eq!(harness.transport.count(), 0, "频道未点名不该回复");

    event.data = json!({
        "id": "msg-2",
        "content": "<@!bot1> /echo 频道",
        "channel_id": "c1",
        "author": { "id": "42", "username": "频道用户" },
        "mentions": [{ "id": "bot1", "is_you": true }]
    });
    let report = harness.pipeline().handle(&event).await;
    assert_eq!(report.outcome, Outcome::Replied);
    let sent = harness.transport.contents();
    assert_eq!(sent[0], "echo: 频道", "艾特标记要被剥掉");
}

#[tokio::test]
async fn inbox_failure_is_logged_and_does_not_block_reply() {
    struct BrokenInbox;

    impl InboxArchive for BrokenInbox {
        fn save(&self, _batch: InboxBatch) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move { Err(String::from("数据库连接断了")) })
        }
    }

    let transport = FakeTransport::new();
    let logger = test_logger();
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_inbox(Arc::new(BrokenInbox) as Arc<dyn InboxArchive>)
        .with_logger(Arc::clone(&logger))
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let report = bot
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/echo hi"))
        .await;
    assert_eq!(report.outcome, Outcome::Replied, "归档失败不影响回复");
    assert!(!report.inbox_recorded);
    assert_eq!(transport.count(), 1);
    assert!(log_contains(&logger, "收件箱归档失败"));
}

#[tokio::test]
async fn non_message_events_are_ignored() {
    let harness = Harness::build(test_config());
    let event = ProtocolEvent {
        id: Some(String::from("evt-x")),
        op: 0,
        event_type: Some(String::from("INTERACTION_CREATE")),
        seq: Some(1),
        data: json!({ "id": "i1" }),
        raw: String::new(),
    };
    let report = harness.pipeline().handle(&event).await;
    assert_eq!(report.outcome, Outcome::Ignored);
    assert_eq!(report.category, EventCategory::Interaction);
    assert_eq!(harness.transport.count(), 0);
}

#[tokio::test]
async fn stats_delta_carries_counts() {
    struct CapturingStats {
        deltas: Mutex<Vec<StatsDelta>>,
    }

    impl StatsRecorder for CapturingStats {
        fn record(&self, delta: StatsDelta) -> BoxFuture<'_, Result<(), String>> {
            self.deltas.lock().unwrap().push(delta);
            Box::pin(async move { Ok(()) })
        }
    }

    let stats = Arc::new(CapturingStats {
        deltas: Mutex::new(Vec::new()),
    });
    let bot = Bot::builder(test_config())
        .with_transport(FakeTransport::new() as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::clone(&stats) as Arc<dyn StatsRecorder>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let report = bot
        .pipeline()
        .handle(&group_event("evt-1", "msg-1", "/echo hi"))
        .await;
    assert!(report.stats_recorded);
    {
        let deltas = stats.deltas.lock().unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].received, 1);
        assert_eq!(deltas[0].sent, 1);
        assert_eq!(deltas[0].conversation, "g1");
        assert_eq!(
            deltas[0].name, "",
            "群聊不该把发送者昵称写成会话名, 群名由群资料接口回填"
        );
        assert_eq!(deltas[0].day.len(), 10, "日期键必须是 YYYY-MM-DD");
    }
    let cis = bot
        .pipeline()
        .handle(&c2c_event("evt-2", "msg-2", "/echo hi"))
        .await;
    assert!(cis.stats_recorded, "单聊也要记统计");
    let deltas = stats.deltas.lock().unwrap();
    assert_eq!(deltas.len(), 2);
    assert_eq!(deltas[1].name, "小红", "单聊要用对方昵称当会话名");
}

fn c2c_event(event_id: &str, message_id: &str, content: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(event_id.to_string()),
        op: 0,
        event_type: Some(String::from("C2C_MESSAGE_CREATE")),
        seq: Some(3),
        data: json!({
            "id": message_id,
            "content": content,
            "author": { "user_openid": "u9", "username": "小红" }
        }),
        raw: String::new(),
    }
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("要能探测空闲端口");
    let port = listener.local_addr().expect("本地地址").port();
    drop(listener);
    port
}

async fn admin_request(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &str,
) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("管理台要能连上");
    let auth = match token {
        Some(token) => format!("Authorization: Bearer {token}\r\n"),
        None => String::new(),
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("请求要能写出");
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await.expect("响应要能读回");
    String::from_utf8_lossy(&buffer).to_string()
}

fn response_body(response: &str) -> &str {
    match response.find("\r\n\r\n") {
        Some(index) => &response[index + 4..],
        None => "",
    }
}

#[tokio::test]
async fn admin_http_control_endpoint_stops_the_bot() {
    let port = free_port();
    let mut config = test_config().with_admin("127.0.0.1", port);
    config.admin_password = String::from("pwd-test");
    let gateway = FakeGateway::new();
    let bot = Bot::builder(config)
        .with_transport(FakeTransport::new() as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_gateway_transport(Arc::clone(&gateway) as Arc<dyn Transport>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let handle = bot.start().await.expect("启动失败");
    wait_until(|| handle.gateway_state() == GatewayState::Ready).await;

    let anonymous = admin_request(port, "POST", "/admin/api/system/stop", None, "").await;
    assert!(
        anonymous.starts_with("HTTP/1.1 401"),
        "未登录要 401: {anonymous}"
    );

    let login = admin_request(
        port,
        "POST",
        "/admin/api/login",
        None,
        r#"{"password":"pwd-test"}"#,
    )
    .await;
    assert!(login.starts_with("HTTP/1.1 200"), "登录要成功: {login}");
    let payload: serde_json::Value =
        serde_json::from_str(response_body(&login).trim()).expect("登录响应要是 JSON");
    let token = payload["token"]
        .as_str()
        .expect("登录要给 token")
        .to_string();

    let accepted = admin_request(port, "POST", "/admin/api/system/stop", Some(&token), "").await;
    assert!(
        accepted.starts_with("HTTP/1.1 202"),
        "登录后要受理: {accepted}"
    );
    let command = tokio::time::timeout(EVENT_WAIT, handle.wait())
        .await
        .expect("管理台 stop 必须结束事件循环");
    assert_eq!(command, ControlCommand::Stop);
    let report = bot
        .shutdown(Duration::from_secs(3))
        .await
        .expect("停机失败");
    assert_eq!(report.live_tasks, 0, "管理台任务也要退干净");
}

#[tokio::test]
async fn plugin_attachment_goes_through_channel_image_bytes() {
    let harness = Harness::build(test_config());
    let mut event = group_event("evt-card", "msg-card", "/card");
    event.event_type = Some(String::from("MESSAGE_CREATE"));
    event.data = json!({
        "id": "msg-card",
        "content": "<@!bot1> /card",
        "channel_id": "c1",
        "author": { "id": "42", "username": "频道用户" },
        "mentions": [{ "id": "bot1", "is_you": true }]
    });
    let report = harness.pipeline().handle(&event).await;
    assert_eq!(report.outcome, Outcome::Replied, "{:?}", report);
    assert!(report.rendered, "附件回复要标记为已渲染");
    let images = harness.transport.images();
    assert_eq!(images.len(), 1, "频道场景走字节直传");
    let (filename, mime, len, msg_id) = images[0].clone();
    assert_eq!(filename, "card.png");
    assert_eq!(mime, "image/png");
    assert_eq!(len, 4);
    assert_eq!(msg_id.as_deref(), Some("msg-card"), "被动发图要带原消息 id");
    assert_eq!(
        harness.transport.captions(),
        vec![String::from("成绩卡")],
        "频道发图必须保留正文 caption"
    );
    assert_eq!(harness.transport.count(), 0, "有附件时不再发文本");
}

#[tokio::test]
async fn plugin_attachment_uploads_then_sends_media_in_group() {
    let harness = Harness::build(test_config());
    let report = harness
        .pipeline()
        .handle(&group_event("evt-card", "msg-card", "/card"))
        .await;
    assert_eq!(report.outcome, Outcome::Replied, "{:?}", report);
    assert!(harness.transport.images().is_empty(), "群聊不走频道直传");
    let media = harness.transport.media();
    assert_eq!(media.len(), 2, "群聊先发正文再上传媒体");
    assert_eq!(media[0].content, "成绩卡", "正文要先发出去");
    assert!(media[0].media.is_none(), "正文不带 file_info");
    assert_eq!(media[0].kind, aurorix_protocol::MessageKind::Markdown);
    assert!(media[0].keyboard.is_some(), "群聊正文要带插件按钮板");
    assert!(media[1].media.is_some(), "媒体消息必须带 file_info");
}

#[tokio::test]
async fn plugin_keyboard_is_attached_to_text_reply() {
    let harness = Harness::build(test_config());
    let mut event = group_event("evt-kb", "msg-kb", "/card");
    event.data = json!({
        "id": "msg-kb",
        "content": "/card",
        "group_openid": "g1",
        "author": { "member_openid": "u1", "username": "小明", "member_role": "member" }
    });
    let report = harness.pipeline().handle(&event).await;
    assert_eq!(report.outcome, Outcome::Replied);
    let media = harness.transport.media();
    assert_eq!(media.len(), 2, "正文与媒体各一条");
    let keyboard = media[0].keyboard.clone().expect("按钮板要挂上");
    assert_eq!(
        keyboard["content"]["rows"][0]["buttons"][0]["id"],
        json!("demo.again"),
        "按钮 id 要带插件前缀"
    );
}

#[tokio::test]
async fn empty_attachment_falls_back_to_pure_text() {
    let harness = Harness::build(test_config());
    let report = harness
        .pipeline()
        .handle(&group_event("evt-empty", "msg-empty", "/empty_card"))
        .await;
    assert_eq!(report.outcome, Outcome::Replied);
    assert_eq!(harness.transport.count(), 1, "空附件要退回文本");
    assert!(harness.transport.images().is_empty());
    assert_eq!(harness.transport.contents(), vec!["只有文字".to_string()]);
}

#[tokio::test]
async fn starting_the_same_bot_twice_is_rejected() {
    let gateway = FakeGateway::new();
    let transport = FakeTransport::new();
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_gateway_transport(Arc::clone(&gateway) as Arc<dyn Transport>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    bot.start().await.expect("首次启动应成功");
    let second = bot.start().await;
    assert!(second.is_err(), "同一个 Bot 不允许启动两次");
    match second.expect_err("错误") {
        aurorix_bot::BotError::AlreadyStarted(text) => {
            assert!(text.contains("只允许启动一次"), "{text}")
        }
        other => panic!("期望 AlreadyStarted, 实际 {other:?}"),
    }
    let report = bot
        .shutdown(Duration::from_secs(5))
        .await
        .expect("关停应成功");
    assert_eq!(report.live_tasks, 0, "关停后不许留下后台任务");
}

#[tokio::test]
async fn runtime_loop_logs_completed_message_events() {
    let gateway = FakeGateway::new();
    let transport = FakeTransport::new();
    let logger = test_logger();
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_gateway_transport(Arc::clone(&gateway) as Arc<dyn Transport>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(Arc::clone(&logger))
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let handle = bot.handle();
    let serving = tokio::spawn(async move { bot.serve().await });
    wait_until(|| handle.gateway_state() == GatewayState::Ready).await;
    gateway.push_text(&event_frame("evt-log", "msg-log", "/echo 记录"));
    wait_until(|| transport.count() == 1).await;
    handle.request_stop();
    tokio::time::timeout(Duration::from_secs(5), serving)
        .await
        .expect("serve 必须退出")
        .expect("serve 不该 panic")
        .expect("serve 不该报错");
    let lines: Vec<String> = logger
        .recent(256)
        .iter()
        .map(|record| record.msg.clone())
        .collect();
    assert!(
        lines.iter().any(|line| line.contains("事件处理完成")),
        "消息事件必须由运行循环记录: {lines:?}"
    );
}

struct ImageRenderer;

impl Renderer for ImageRenderer {
    fn render<'a>(
        &'a self,
        _card: &'a aurorix_bot::CardView,
    ) -> BoxFuture<'a, Result<Rendered, RenderError>> {
        Box::pin(async move {
            Ok(Rendered::Image {
                bytes: vec![1, 2, 3, 4],
                filename: String::from("card.webp"),
            })
        })
    }

    fn render_card<'a>(
        &'a self,
        _card: &'a aurorix_plugin::RenderCard,
    ) -> BoxFuture<'a, Result<Rendered, RenderError>> {
        Box::pin(async move {
            Ok(Rendered::Image {
                bytes: vec![1, 2, 3, 4],
                filename: String::from("card.webp"),
            })
        })
    }
}

struct FakeImageStore {
    url: String,
}

impl aurorix_api::ImageStore for FakeImageStore {
    fn ensure(&self, url: &str) -> BoxFuture<'_, Result<String, ApiError>> {
        let url = url.to_string();
        Box::pin(async move { Ok(url) })
    }

    fn store(
        &self,
        bytes: &[u8],
        filename: &str,
    ) -> BoxFuture<'_, Result<aurorix_api::StoredImage, ApiError>> {
        assert_eq!(bytes, [1, 2, 3, 4], "卡片字节要原样交给图床");
        assert_eq!(filename, "card.webp", "文件名要带上扩展名");
        let url = self.url.clone();
        Box::pin(async move {
            let target = aurorix_bot::images::current_target();
            assert_eq!(
                target.id, "g1",
                "图床上传要带上群目标, 否则官方上传拿不到 openid"
            );
            Ok(aurorix_api::StoredImage {
                url,
                width: 900,
                height: 1200,
            })
        })
    }
}

#[tokio::test]
async fn markdown_mode_embeds_card_as_image_url() {
    let transport = FakeTransport::new();
    let mut config = test_config();
    config.global_markdown = true;
    let bot = Bot::builder(config)
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_render(Arc::new(ImageRenderer) as Arc<dyn Renderer>)
        .with_images(Arc::new(FakeImageStore {
            url: String::from("https://img.example/card.webp"),
        }))
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let report = bot
        .pipeline()
        .handle(&group_event("evt-md", "msg-md", "/card_page"))
        .await;
    assert!(report.sent, "markdown 模式下卡片要发出去");
    let sent = transport.contents();
    assert!(
        sent[0].contains("![图片 #900px #1200px](https://img.example/card.webp)"),
        "群聊卡片要转成 markdown 图片链接: {}",
        sent[0]
    );
    assert!(
        transport.uploads().is_empty(),
        "markdown 成功时不该再走富媒体上传"
    );
    let _ = bot.shutdown(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn inbox_records_platform_id_and_refreshes_conversation_name() {
    let transport = FakeTransport::new();
    transport.provide_name("测试群");
    let inbox = Arc::new(aurorix_bot::MemoryInbox::new());
    let stats = Arc::new(MemoryStats::new());
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::clone(&stats) as Arc<dyn StatsRecorder>)
        .with_inbox(Arc::clone(&inbox) as Arc<dyn aurorix_bot::InboxArchive>)
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let first = bot
        .pipeline()
        .handle(&group_event("evt-name-1", "msg-name-1", "/echo 你好"))
        .await;
    assert!(first.inbox_recorded, "首条消息要写进收件箱");
    assert_eq!(
        inbox.names().get("g1").map(String::as_str),
        Some("测试群"),
        "群名要回填到会话上"
    );
    assert_eq!(
        stats.names().get("g1").map(String::as_str),
        Some("测试群"),
        "统计表也要用群名, 不能拿发送者昵称当会话名"
    );
    let messages = inbox.messages();
    let outbound = messages
        .iter()
        .find(|message| message.direction == aurorix_db::Direction::Outbound)
        .expect("机器人回复也要入库");
    assert_eq!(
        outbound.platform_id, "out-1",
        "出站消息要记住平台消息 id, 否则页面拿不到回执"
    );
    assert_eq!(
        outbound.content, "echo: 你好",
        "出站消息要存真正发出去的正文"
    );
    let _ = bot
        .pipeline()
        .handle(&group_event("evt-name-2", "msg-name-2", "/echo 再来"))
        .await;
    assert_eq!(
        transport.name_calls(),
        1,
        "同一会话的群名不该每条消息都查一次"
    );
    let _ = bot.shutdown(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn midway_send_arrives_before_the_final_reply() {
    let harness = Harness::build(test_config());
    let report = harness
        .pipeline()
        .handle(&group_event(
            "evt-progress-1",
            "msg-progress-1",
            "/progress",
        ))
        .await;
    assert_eq!(report.outcome, Outcome::Replied);
    assert_eq!(report.reply_text.as_deref(), Some("查询完成"));
    assert_eq!(
        harness.transport.contents(),
        vec![String::from("正在查询…"), String::from("查询完成")],
        "先行消息必须先发出, 最终回复必须后发出"
    );
    let outbound: Vec<String> = harness
        .inbox
        .messages()
        .into_iter()
        .filter(|message| message.direction == aurorix_db::Direction::Outbound)
        .map(|message| message.content)
        .collect();
    assert_eq!(
        outbound,
        vec![String::from("正在查询…"), String::from("查询完成")],
        "两条回复都要落库并且顺序一致"
    );
    let inbound = harness
        .inbox
        .messages()
        .into_iter()
        .filter(|message| message.direction == aurorix_db::Direction::Inbound)
        .count();
    assert_eq!(inbound, 1, "用户消息只能入库一次");
    assert_eq!(harness.stats.calls(), 1, "统计仍按一次指令计数");
}

fn markdown_config() -> BotConfig {
    let mut config = test_config();
    config.global_markdown = true;
    config
}

fn group_target() -> Target {
    Target::new(aurorix_core::Scene::Group, "g1").with_user("u1")
}

#[tokio::test]
async fn chain_keeps_many_image_urls_in_order() {
    let harness = Harness::build(markdown_config());
    let reply = Outgoing::new(ReplyKind::Handler, "")
        .image_url("https://img.example/a.png", "第一张")
        .image_url("https://img.example/b.png", "第二张");
    let target = group_target();
    let report = harness.pipeline().push(&target, target.scene, &reply).await;
    assert!(report.sent, "{report:?}");
    let contents = harness.transport.contents();
    assert_eq!(contents.len(), 1, "两张图都内嵌进同一条正文");
    assert_eq!(
        contents[0],
        "![第一张](https://img.example/a.png)![第二张](https://img.example/b.png)"
    );
    let first = contents[0].find("a.png").expect("第一张要在正文里");
    let second = contents[0].find("b.png").expect("第二张要在正文里");
    assert!(first < second, "图片顺序必须和链上顺序一致");
}

#[tokio::test]
async fn text_image_markdown_keep_chain_order() {
    let harness = Harness::build(markdown_config());
    let reply = Outgoing::new(ReplyKind::Handler, "A")
        .image_url("https://img.example/mid.png", "图")
        .markdown("B");
    let target = group_target();
    let report = harness.pipeline().push(&target, target.scene, &reply).await;
    assert!(report.sent, "{report:?}");
    let contents = harness.transport.contents();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0], "A![图](https://img.example/mid.png)B");
    assert!(contents[0].starts_with("A"), "{}", contents[0]);
    assert!(contents[0].ends_with('B'), "{}", contents[0]);
}

#[tokio::test]
async fn failed_card_render_uses_its_own_fallback() {
    let transport = FakeTransport::new();
    let (renderer, _calls) = FakeRenderer::new(true);
    let logger = test_logger();
    let bot = Bot::builder(test_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_render(renderer as Arc<dyn Renderer>)
        .with_logger(Arc::clone(&logger))
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let card = aurorix_plugin::RenderCard::new(
        aurorix_render::RenderRequest::new(64, 64),
        "test.png",
        |_kit| {},
    );
    let reply = Outgoing::new(ReplyKind::Handler, "").card_or(card, "文字页");
    let target = group_target();
    let report = bot.pipeline().push(&target, target.scene, &reply).await;
    assert!(report.sent, "{report:?}");
    assert!(!report.rendered, "渲染失败不算已渲染");
    assert_eq!(transport.contents(), vec![String::from("文字页")]);
    assert!(log_contains(&logger, "卡片渲染失败"), "渲染失败要留下日志");
    let _ = bot.shutdown(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn guild_image_only_drops_the_caption() {
    let harness = Harness::build(test_config());
    let reply = Outgoing::new(ReplyKind::Handler, "成绩图")
        .image(aurorix_plugin::Attachment::png(vec![1, 2, 3, 4]))
        .image_only();
    let target = Target::new(aurorix_core::Scene::Channel, "c1");
    let report = harness.pipeline().push(&target, target.scene, &reply).await;
    assert!(report.sent, "{report:?}");
    assert_eq!(
        harness.transport.captions(),
        vec![String::new()],
        "image_only 要把正文让给图片"
    );
    assert!(harness.transport.contents().is_empty(), "有图就不再发文本");
}

#[tokio::test]
async fn avatar_part_embeds_the_avatar_markup() {
    let harness = Harness::build(test_config());
    let reply = Outgoing::new(ReplyKind::Handler, "看这里").at_avatar("u9", 40);
    let target = group_target();
    let report = harness.pipeline().push(&target, target.scene, &reply).await;
    assert!(report.sent, "{report:?}");
    let contents = harness.transport.contents();
    assert_eq!(contents.len(), 1);
    assert_eq!(
        contents[0],
        "看这里![cornerRadius=100 #40px #40px](https://thirdqq.qlogo.cn/qqapp/appid-test/u9/140) <qqbot-at-user id=\"u9\" />"
    );
}

#[tokio::test]
async fn image_and_voice_send_main_message_first() {
    let transport = FakeTransport::new();
    let bot = Bot::builder(markdown_config())
        .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(registry_with(Arc::new(Mutex::new(Vec::new()))))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_images(Arc::new(FakeImageStore {
            url: String::from("https://img.example/card.webp"),
        }))
        .with_logger(test_logger())
        .with_signal_handling(false)
        .with_instance_lock(false)
        .build()
        .expect("装配失败");
    let reply = Outgoing::new(ReplyKind::Handler, "正文")
        .image(aurorix_plugin::Attachment::new(
            vec![1, 2, 3, 4],
            "card.webp",
            "image/webp",
        ))
        .voice(aurorix_plugin::Media::bytes(vec![9, 9], "voice.silk"));
    let target = group_target();
    let report = bot.pipeline().push(&target, target.scene, &reply).await;
    assert!(report.sent, "{report:?}");
    let contents = transport.contents();
    assert_eq!(contents.len(), 2, "主消息和语音各一条");
    assert!(
        contents[0].contains("![图片 #900px #1200px](https://img.example/card.webp)"),
        "{}",
        contents[0]
    );
    let media = transport.media();
    assert_eq!(media.len(), 2);
    assert!(media[0].media.is_none(), "第一条是主消息");
    assert!(media[1].media.is_some(), "第二条才是语音");
    assert_eq!(
        transport.media_uploads(),
        vec![aurorix_plugin::MEDIA_VOICE],
        "语音要走媒体上传"
    );
    let _ = bot.shutdown(Duration::from_secs(2)).await;
}
