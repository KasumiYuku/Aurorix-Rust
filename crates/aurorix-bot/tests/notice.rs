use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_api::{ApiError, Outbound, SendOutcome};
use aurorix_bot::{
    Bot, BotConfig, InboxArchive, KvBackend, Logger, MemoryInbox, MemoryKv, MemoryStats, Outcome,
    Pipeline, ReplyTransport, StatsRecorder,
};
use aurorix_core::Target;
use aurorix_gateway::{BoxFuture, ProtocolEvent, StaticTokenSource, TokenSource};
use aurorix_log::{Level, Options};
use aurorix_protocol::MessageKind;
use serde_json::json;

struct FakeTransport {
    sent: Mutex<Vec<(Target, Outbound)>>,
}

impl FakeTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
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

    fn count(&self) -> usize {
        self.sent.lock().unwrap().len()
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
            let index = self.count() + 1;
            self.sent
                .lock()
                .unwrap()
                .push((target.clone(), message.clone()));
            Ok(SendOutcome {
                message_id: Some(format!("out-{index}")),
                audit_id: None,
                target,
                delivered_active: message.initiative,
                fallback: false,
            })
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
        let kv = Arc::new(MemoryKv::new());
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
            .with_kv(Arc::clone(&kv) as Arc<dyn KvBackend>)
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
    config.welcome_messages = Some(vec![String::from("欢迎来到这里")]);
    config.farewell_messages = Some(vec![String::from("有人离开了群聊")]);
    config
}

fn member_event(event_type: &str, event_id: &str, user: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(event_id.to_string()),
        op: 0,
        event_type: Some(event_type.to_string()),
        seq: Some(5),
        data: json!({
            "group_openid": "g1",
            "member_openid": user,
            "timestamp": 1_700_000_000
        }),
        raw: String::new(),
    }
}

fn receive_event(event_type: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(String::from("evt-gate")),
        op: 0,
        event_type: Some(event_type.to_string()),
        seq: Some(6),
        data: json!({ "group_openid": "g1", "timestamp": 1_700_000_001 }),
        raw: String::new(),
    }
}

#[tokio::test]
async fn member_add_sends_a_passive_welcome_with_avatar_and_mention() {
    let harness = Harness::build(config());
    let report = harness
        .pipeline
        .handle(&member_event("GROUP_MEMBER_ADD", "evt-welcome", "u1"))
        .await;
    assert_eq!(report.outcome, Outcome::Notified);
    assert!(report.sent, "入群欢迎必须真的发出去");
    assert!(report.inbox_recorded, "出站消息要归档");
    let outbound = harness.transport.outbound();
    assert_eq!(outbound.len(), 1);
    assert_eq!(
        outbound[0].kind,
        MessageKind::Markdown,
        "欢迎消息走 markdown"
    );
    assert_eq!(
        outbound[0].anchor.event_id, "evt-welcome",
        "入群事件有 event_id, 应当走被动回复"
    );
    assert!(!outbound[0].initiative, "被动回复不算主动消息");
    let content = &outbound[0].content;
    assert!(
        content.starts_with("##![cornerRadius=100 #40px #40px](https://thirdqq.qlogo.cn/qqapp/appid-test/u1/140) <@u1>"),
        "标题行要带头像与艾特, 实际是 {content}"
    );
    assert!(content.ends_with("欢迎来到这里"));
    let batches = harness.inbox.saved();
    assert_eq!(batches.len(), 1, "通知只归档一次");
    assert_eq!(batches[0].len(), 1, "通知只落一条出站消息");
    assert_eq!(batches[0].messages[0].platform_id, "out-1");
}

#[tokio::test]
async fn member_remove_uses_initiative_and_skips_mention() {
    let harness = Harness::build(config());
    let report = harness
        .pipeline
        .handle(&member_event("GROUP_MEMBER_REMOVE", "evt-bye", "u2"))
        .await;
    assert_eq!(report.outcome, Outcome::Notified);
    let outbound = harness.transport.outbound();
    assert_eq!(outbound.len(), 1);
    assert!(outbound[0].initiative, "退群只能主动推送");
    assert!(
        outbound[0].anchor.event_id.is_empty(),
        "主动推送不该带 passive 凭据"
    );
    assert!(
        !outbound[0].content.contains("<@u2>"),
        "退群的人已经不在群里, 不该艾特"
    );
    assert!(outbound[0]
        .content
        .contains("https://thirdqq.qlogo.cn/qqapp/appid-test/u2/140"));
    assert!(outbound[0].content.ends_with("有人离开了群聊"));
}

#[tokio::test]
async fn rejected_receive_permission_blocks_member_notice() {
    let harness = Harness::build(config());
    let gate = harness
        .pipeline
        .handle(&receive_event("GROUP_MSG_REJECT"))
        .await;
    assert_eq!(gate.outcome, Outcome::Ignored);
    let report = harness
        .pipeline
        .handle(&member_event("GROUP_MEMBER_REMOVE", "evt-bye", "u3"))
        .await;
    assert_eq!(report.outcome, Outcome::Ignored);
    assert_eq!(
        harness.transport.count(),
        0,
        "群关掉主动消息后不该再推送退群提示"
    );
    assert!(
        harness
            .logs()
            .iter()
            .any(|line| line.contains("未开启主动消息")),
        "要留下跳过原因"
    );
}

#[tokio::test]
async fn granted_receive_permission_allows_the_push_again() {
    let harness = Harness::build(config());
    harness
        .pipeline
        .handle(&receive_event("GROUP_MSG_REJECT"))
        .await;
    harness
        .pipeline
        .handle(&receive_event("GROUP_MSG_RECEIVE"))
        .await;
    let report = harness
        .pipeline
        .handle(&member_event("GROUP_MEMBER_REMOVE", "evt-bye", "u4"))
        .await;
    assert_eq!(report.outcome, Outcome::Notified);
    assert_eq!(harness.transport.count(), 1, "开关重新打开后要能推送");
}

#[tokio::test]
async fn disabled_notice_switch_sends_nothing() {
    let mut config = config();
    config.welcome_enabled = Some(false);
    let harness = Harness::build(config);
    let report = harness
        .pipeline
        .handle(&member_event("GROUP_MEMBER_ADD", "evt-welcome", "u5"))
        .await;
    assert_eq!(report.outcome, Outcome::Ignored);
    assert_eq!(harness.transport.count(), 0);
}

#[tokio::test]
async fn empty_notice_lines_send_nothing() {
    let mut config = config();
    config.welcome_messages = Some(vec![String::from("   "), String::new()]);
    let harness = Harness::build(config);
    let report = harness
        .pipeline
        .handle(&member_event("GROUP_MEMBER_ADD", "evt-welcome", "u6"))
        .await;
    assert_eq!(report.outcome, Outcome::Ignored);
    assert_eq!(harness.transport.count(), 0, "文案全是空白时不该发消息");
}

#[tokio::test]
async fn friend_add_welcomes_the_new_contact() {
    let harness = Harness::build(config());
    let event = ProtocolEvent {
        id: Some(String::from("evt-friend")),
        op: 0,
        event_type: Some(String::from("FRIEND_ADD")),
        seq: Some(7),
        data: json!({ "openid": "u8", "timestamp": 1_700_000_002 }),
        raw: String::new(),
    };
    let report = harness.pipeline.handle(&event).await;
    assert_eq!(report.outcome, Outcome::Notified);
    let outbound = harness.transport.outbound();
    assert_eq!(outbound.len(), 1, "加好友要发一条欢迎");
    assert_eq!(outbound[0].kind, MessageKind::Markdown);
    assert!(outbound[0].content.contains("<@u8>"), "好友欢迎也要艾特");
}

#[tokio::test]
async fn unexpected_notice_shapes_are_ignored_without_panicking() {
    let harness = Harness::build(config());
    let event = ProtocolEvent {
        id: Some(String::from("evt-bad")),
        op: 0,
        event_type: Some(String::from("GROUP_MEMBER_ADD")),
        seq: Some(8),
        data: json!({ "group_openid": "g1" }),
        raw: String::new(),
    };
    let report = harness.pipeline.handle(&event).await;
    assert_eq!(report.outcome, Outcome::Ignored);
    assert_eq!(harness.transport.count(), 0);
    let _ = Duration::from_millis(0);
}
