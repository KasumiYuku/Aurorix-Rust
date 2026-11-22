use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use aurorix_api::{ApiError, MediaResult, Outbound, SendOutcome};
use aurorix_bot::{
    Bot, BotConfig, InboxArchive, Logger, MemoryInbox, MemoryStats, Pipeline, ReplyTransport,
    StatsRecorder,
};
use aurorix_core::{Scene, Target};
use aurorix_gateway::{BoxFuture, StaticTokenSource, TokenSource};
use aurorix_log::{Level, Options};
use aurorix_plugin::{Attachment, Outgoing, ReplyKind};
use serde_json::json;

struct FakeTransport {
    sent: Mutex<Vec<(Target, Outbound)>>,
    uploaded: Mutex<Vec<String>>,
    images: AtomicUsize,
    fail: Mutex<Option<String>>,
}

impl FakeTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
            uploaded: Mutex::new(Vec::new()),
            images: AtomicUsize::new(0),
            fail: Mutex::new(None),
        })
    }

    fn outbound(&self) -> Vec<Outbound> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|(_, message)| message.clone())
            .collect()
    }

    fn fail_with(&self, reason: &str) {
        *self.fail.lock().unwrap() = Some(reason.to_string());
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
            if let Some(reason) = self.fail.lock().unwrap().clone() {
                return Err(ApiError::Config(reason));
            }
            let index = self.sent.lock().unwrap().len() + 1;
            self.sent
                .lock()
                .unwrap()
                .push((target.clone(), message.clone()));
            Ok(SendOutcome {
                message_id: Some(format!("push-{index}")),
                audit_id: None,
                target,
                delivered_active: message.initiative,
                fallback: false,
            })
        })
    }

    fn upload_image<'a>(
        &'a self,
        _target: &'a Target,
        _data: Vec<u8>,
        filename: String,
    ) -> BoxFuture<'a, Result<MediaResult, ApiError>> {
        Box::pin(async move {
            self.uploaded.lock().unwrap().push(filename);
            Ok(MediaResult {
                file_info: String::from("file-info-1"),
                file_uuid: String::from("uuid-1"),
                ttl: 3600,
                raw_url: String::from("https://example.test/card.png"),
            })
        })
    }

    fn send_image_bytes<'a>(
        &'a self,
        target: &'a Target,
        _content: String,
        _data: Vec<u8>,
        _filename: String,
        _mime_type: String,
        _msg_id: Option<String>,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        let target = target.clone();
        Box::pin(async move {
            self.images.fetch_add(1, Ordering::SeqCst);
            self.sent.lock().unwrap().push((
                target,
                Outbound::media(String::new(), json!({ "file_info": "channel-image" })),
            ));
            Ok(Some(String::from("channel-1")))
        })
    }
}

struct Harness {
    pipeline: Arc<Pipeline>,
    transport: Arc<FakeTransport>,
    inbox: Arc<MemoryInbox>,
    logger: Arc<Logger>,
}

impl Harness {
    fn build(config: BotConfig) -> Self {
        let transport = FakeTransport::new();
        let inbox = Arc::new(MemoryInbox::new());
        let logger = Logger::new(
            Options::default()
                .with_console(false)
                .with_level(Level::Debug),
        );
        let bot = Bot::builder(config)
            .with_transport(Arc::clone(&transport) as Arc<dyn ReplyTransport>)
            .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
            .with_plugins(Arc::new(aurorix_plugin::CommandRegistry::new()))
            .with_inbox(Arc::clone(&inbox) as Arc<dyn InboxArchive>)
            .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
            .with_logger(Arc::clone(&logger))
            .with_signal_handling(false)
            .with_instance_lock(false)
            .build()
            .expect("测试机器人要能装配");
        Self {
            pipeline: Arc::clone(bot.pipeline()),
            transport,
            inbox,
            logger,
        }
    }

    fn logs(&self) -> Vec<String> {
        self.logger
            .recent(64)
            .iter()
            .map(|record| format!("{} {:?}", record.msg, record.fields))
            .collect()
    }
}

fn config() -> BotConfig {
    let mut config = BotConfig::new("appid-test", "secret-test");
    config.global_markdown = true;
    config
}

fn group() -> Target {
    Target::new(Scene::Group, "g1").with_user("u1")
}

fn channel() -> Target {
    Target::new(Scene::Channel, "c1")
}

#[tokio::test]
async fn group_push_is_initiative_markdown_with_mention_and_quote() {
    let harness = Harness::build(config());
    let reply = Outgoing::new(ReplyKind::Handler, "打卡完成")
        .mention_user("u9")
        .reply_to("m5");
    let report = harness.pipeline.push(&group(), group().scene, &reply).await;
    assert!(report.sent, "主动推送要成功: {report:?}");
    assert!(report.archived, "出站消息要归档");
    assert_eq!(report.platform_id, "push-1");
    let outbound = &harness.transport.outbound()[0];
    assert!(outbound.initiative, "主动推送必须标记为非被动消息");
    assert_eq!(
        outbound.content,
        "> ### ![cornerRadius=100 #30px #30px](https://thirdqq.qlogo.cn/qqapp/appid-test/u9/140) <@u9>\n打卡完成",
        "要带上带头像的艾特"
    );
    assert_eq!(outbound.reference.as_deref(), Some("m5"), "要带上引用回复");
    assert_eq!(outbound.kind, aurorix_protocol::MessageKind::Markdown);
}

#[tokio::test]
async fn channel_push_uses_plain_text_without_mention() {
    let harness = Harness::build(config());
    let reply = Outgoing::new(ReplyKind::Handler, "频道公告");
    let report = harness
        .pipeline
        .push(&channel(), channel().scene, &reply)
        .await;
    assert!(report.sent, "频道推送要成功: {report:?}");
    let outbound = &harness.transport.outbound()[0];
    assert!(outbound.initiative);
    assert_eq!(outbound.content, "频道公告");
    assert_eq!(outbound.kind, aurorix_protocol::MessageKind::PlainText);
}

#[tokio::test]
async fn image_push_uploads_media() {
    let harness = Harness::build(config());
    let reply =
        Outgoing::new(ReplyKind::Handler, "成绩图").image(Attachment::png(vec![1, 2, 3, 4]));
    let report = harness.pipeline.push(&group(), group().scene, &reply).await;
    assert!(report.sent, "图片推送要成功: {report:?}");
    assert!(report.rendered, "图片推送要标记已渲染");
    assert_eq!(harness.transport.uploaded.lock().unwrap().len(), 1);
    let outbound = harness.transport.outbound();
    assert_eq!(outbound.len(), 2, "markdown 正文与媒体各一条");
    assert!(outbound[0].initiative);
    assert_eq!(outbound[0].content, "成绩图");
    assert_eq!(outbound[0].kind, aurorix_protocol::MessageKind::Markdown);
    assert!(outbound[1].initiative);
    assert_eq!(outbound[1].kind, aurorix_protocol::MessageKind::Media);
    assert_eq!(
        outbound[1].media.as_ref().unwrap()["file_info"],
        json!("file-info-1")
    );
}

#[tokio::test]
async fn channel_image_push_goes_through_image_bytes() {
    let harness = Harness::build(config());
    let reply =
        Outgoing::new(ReplyKind::Handler, "成绩图").image(Attachment::png(vec![9, 9, 9, 9]));
    let report = harness
        .pipeline
        .push(&channel(), channel().scene, &reply)
        .await;
    assert!(report.sent, "频道图片推送要成功: {report:?}");
    assert_eq!(harness.transport.images.load(Ordering::SeqCst), 1);
    assert!(harness.transport.uploaded.lock().unwrap().is_empty());
    assert_eq!(report.text.as_deref(), Some("[频道图片]"));
}

#[tokio::test]
async fn empty_push_is_not_sent() {
    let harness = Harness::build(config());
    let reply = Outgoing::new(ReplyKind::Handler, "   ");
    let report = harness.pipeline.push(&group(), group().scene, &reply).await;
    assert!(!report.sent, "空内容不该发送");
    assert_eq!(harness.transport.outbound().len(), 0);
    assert!(
        harness
            .logs()
            .iter()
            .any(|line| line.contains("主动推送内容为空")),
        "要留下跳过原因: {:?}",
        harness.logs()
    );
}

#[tokio::test]
async fn push_failure_reports_error_and_skips_archive() {
    let harness = Harness::build(config());
    harness.transport.fail_with("主动消息发送失败，无权限");
    let reply = Outgoing::new(ReplyKind::Handler, "打卡完成");
    let report = harness.pipeline.push(&group(), group().scene, &reply).await;
    assert!(!report.sent, "失败时不能算已发送");
    assert!(!report.archived, "失败时不该归档");
    let error = report.error.expect("要带回错误原因");
    assert!(error.contains("无权限"), "错误要保留平台原因: {error}");
    assert!(
        harness
            .logs()
            .iter()
            .any(|line| line.contains("主动消息发送失败")),
        "要留下失败日志: {:?}",
        harness.logs()
    );
}

#[tokio::test]
async fn push_archives_outbound_message_only() {
    let harness = Harness::build(config());
    let reply = Outgoing::new(ReplyKind::Handler, "定时提醒");
    harness.pipeline.push(&group(), group().scene, &reply).await;
    let rows = harness.inbox.messages();
    assert_eq!(rows.len(), 1, "只归档出站一条, 不伪造入站消息");
    assert_eq!(rows[0].content, "定时提醒");
    assert_eq!(rows[0].platform_id, "push-1");
    assert!(rows[0].sender_id.is_empty(), "出站消息不带发送者");
}
