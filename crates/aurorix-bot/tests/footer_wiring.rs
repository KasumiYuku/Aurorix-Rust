use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use aurorix_api::{ApiError, Outbound, SendOutcome};
use aurorix_bot::{
    BotConfig, KvBackend, Logger, MemoryKv, Outcome, Pipeline, PipelineDeps, ReplyTransport,
};
use aurorix_buttons::{FooterHub, FooterScope};
use aurorix_core::Target;
use aurorix_gateway::{BoxFuture, ProtocolEvent};
use aurorix_log::{Level, Options};
use aurorix_plugin::{CommandRegistry, CommandSpec, Invocation, Outgoing, ReplyKind};
use serde_json::json;

struct RecordingTransport {
    sent: Mutex<Vec<(Target, Outbound)>>,
    acks: Mutex<Vec<String>>,
    failures: AtomicUsize,
}

impl RecordingTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
            acks: Mutex::new(Vec::new()),
            failures: AtomicUsize::new(0),
        })
    }

    fn sent(&self) -> Vec<(Target, Outbound)> {
        self.sent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn contents(&self) -> Vec<String> {
        self.sent()
            .into_iter()
            .map(|(_, outbound)| outbound.content)
            .collect()
    }

    fn acks(&self) -> Vec<String> {
        self.acks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl ReplyTransport for RecordingTransport {
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
                    status: 500,
                    body: String::from("假发送通道故障"),
                });
            }
            let index = self
                .sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len();
            self.sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((target, message));
            Ok(SendOutcome {
                message_id: Some(format!("m{}", index + 1)),
                audit_id: None,
                target: Target::new(aurorix_core::Scene::Group, "g1"),
                delivered_active: true,
                fallback: false,
            })
        })
    }

    fn ack_interaction<'a>(
        &'a self,
        interaction_id: &'a str,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        let id = interaction_id.to_string();
        Box::pin(async move {
            self.acks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(id);
            Ok(())
        })
    }
}

fn logger() -> Arc<Logger> {
    Logger::new(Options::default().with_level(Level::Error))
}

fn hub(lines: &[&str]) -> Arc<FooterHub> {
    let hub = FooterHub::new();
    hub.set_lines(
        FooterScope::Global,
        lines.iter().map(|line| (*line).to_string()).collect(),
    );
    Arc::new(hub)
}

fn pipeline(
    transport: Arc<RecordingTransport>,
    kv: Arc<dyn KvBackend>,
    footers: Arc<FooterHub>,
    registry: &Arc<CommandRegistry>,
) -> Arc<Pipeline> {
    let mut deps = PipelineDeps::new(
        Arc::new(BotConfig::new("1000", "secret")),
        Arc::new(aurorix_plugin::Router::new(Arc::clone(registry))),
        transport,
        logger(),
    );
    deps.kv = Some(kv);
    deps.footers = Some(footers);
    Pipeline::new(deps)
}

fn empty_registry() -> Arc<CommandRegistry> {
    Arc::new(CommandRegistry::new())
}

fn click(button_data: &str, user: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(String::from("ev-int")),
        op: 0,
        event_type: Some(String::from("INTERACTION_CREATE")),
        seq: Some(7),
        data: json!({
            "id": "int-1",
            "type": 11,
            "scene": "group",
            "chat_type": 0,
            "group_openid": "g1",
            "data": {
                "type": 11,
                "resolved": {
                    "button_data": button_data,
                    "button_id": format!("aurorix.footer.{button_data}"),
                    "user_id": user
                }
            }
        }),
        raw: String::new(),
    }
}

fn group_message(content: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(String::from("ev-1")),
        op: 0,
        event_type: Some(String::from("GROUP_AT_MESSAGE_CREATE")),
        seq: Some(2),
        data: json!({
            "id": "msg-1",
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

#[tokio::test]
async fn counter_click_replies_with_the_count() {
    let transport = RecordingTransport::new();
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let pipeline = pipeline(
        Arc::clone(&transport),
        Arc::clone(&kv),
        hub(&["+1 | counter"]),
        &empty_registry(),
    );
    let report = pipeline.handle(&click("0", "u1")).await;
    assert_eq!(report.outcome, Outcome::Replied, "计数按钮应当产生回复");
    assert!(report.sent, "回复必须真的发出去");
    let contents = transport.contents();
    assert_eq!(contents.len(), 1, "一次点击只应发一条消息");
    assert!(
        contents[0].contains("1"),
        "默认模板会带上当前计数, 实得 {}",
        contents[0]
    );
    assert_eq!(
        transport.acks(),
        vec![String::from("int-1")],
        "必须回执交互, 否则客户端一直转圈"
    );
}

#[tokio::test]
async fn repeat_click_by_the_same_user_is_deduplicated() {
    let transport = RecordingTransport::new();
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let pipeline = pipeline(
        Arc::clone(&transport),
        Arc::clone(&kv),
        hub(&["+1 | counter"]),
        &empty_registry(),
    );
    pipeline.handle(&click("0", "u1")).await;
    pipeline.handle(&click("0", "u1")).await;
    let contents = transport.contents();
    assert_eq!(contents.len(), 2, "两次点击都应回复");
    assert_eq!(
        contents[1], contents[0],
        "默认按用户去重, 同一个人重复点不应把计数加高"
    );
    assert_eq!(
        kv.get("footer.+1@seen-check").await.expect("读取不应失败"),
        None,
        "不存在的键应当返回 None"
    );
}

#[tokio::test]
async fn dedup_none_counts_every_click() {
    let transport = RecordingTransport::new();
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let pipeline = pipeline(
        Arc::clone(&transport),
        Arc::clone(&kv),
        hub(&["+1 | counter | ; dedup=none"]),
        &empty_registry(),
    );
    pipeline.handle(&click("0", "u1")).await;
    pipeline.handle(&click("0", "u1")).await;
    let contents = transport.contents();
    assert_eq!(contents.len(), 2, "两次点击都应回复");
    assert_ne!(
        contents[0], contents[1],
        "不去重时计数必须增长, 两次回复的计数不同"
    );
}

#[tokio::test]
async fn duplicate_reply_uses_the_dedicated_template() {
    let transport = RecordingTransport::new();
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let pipeline = pipeline(
        Arc::clone(&transport),
        Arc::clone(&kv),
        hub(&["+1 | counter | 计 {{count}} ; dup=重复点击 ; dedup=user"]),
        &empty_registry(),
    );
    pipeline.handle(&click("0", "u1")).await;
    pipeline.handle(&click("0", "u1")).await;
    let contents = transport.contents();
    assert_eq!(contents[0], "计 1", "首次点击走主模板");
    assert_eq!(contents[1], "重复点击", "重复点击走 dup 文案");
}

#[tokio::test]
async fn image_button_sends_markdown_image() {
    let transport = RecordingTransport::new();
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let pipeline = pipeline(
        Arc::clone(&transport),
        Arc::clone(&kv),
        hub(&["文档 | image | https://example.com/a.png"]),
        &empty_registry(),
    );
    let report = pipeline.handle(&click("0", "u1")).await;
    assert_eq!(report.outcome, Outcome::Replied);
    assert_eq!(
        transport.contents(),
        vec![String::from("![文档](https://example.com/a.png)")],
        "图片按钮应当发 markdown 内嵌图"
    );
}

#[tokio::test]
async fn click_without_footer_config_is_ignored_quietly() {
    let transport = RecordingTransport::new();
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let mut deps = PipelineDeps::new(
        Arc::new(BotConfig::new("1000", "secret")),
        Arc::new(aurorix_plugin::Router::new(empty_registry())),
        Arc::clone(&transport) as Arc<dyn ReplyTransport>,
        logger(),
    );
    deps.kv = Some(kv);
    let pipeline = Pipeline::new(deps);
    let report = pipeline.handle(&click("0", "u1")).await;
    assert_eq!(
        report.outcome,
        Outcome::Interaction,
        "没有底部按钮配置时不该硬造回复"
    );
    assert!(transport.contents().is_empty(), "不应发送任何消息");
    assert_eq!(transport.acks().len(), 1, "仍然要回执交互");
}

#[tokio::test]
async fn stale_button_index_is_reported_without_sending() {
    let transport = RecordingTransport::new();
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let pipeline = pipeline(
        Arc::clone(&transport),
        Arc::clone(&kv),
        hub(&["+1 | counter"]),
        &empty_registry(),
    );
    let report = pipeline.handle(&click("3", "u1")).await;
    assert_eq!(report.outcome, Outcome::Interaction);
    assert!(
        transport.contents().is_empty(),
        "配置里没有第 4 个按钮, 不应发送任何消息"
    );
}

#[tokio::test]
async fn message_reply_carries_footer_buttons() {
    let transport = RecordingTransport::new();
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let registry = empty_registry();
    registry.register(
        "demo",
        CommandSpec::new("/ping").on_sync(|_invocation: &Invocation| {
            Ok(Some(Outgoing::new(ReplyKind::Handler, "pong")))
        }),
    );
    let pipeline = pipeline(
        Arc::clone(&transport),
        Arc::clone(&kv),
        hub(&["帮助 | command | /help"]),
        &registry,
    );
    let report = pipeline.handle(&group_message("/ping")).await;
    assert_eq!(report.outcome, Outcome::Replied, "指令应当命中");
    let sent = transport.sent();
    assert_eq!(sent.len(), 1, "只应发一条回复");
    assert_eq!(sent[0].1.content, "pong");
    let keyboard = sent[0]
        .1
        .keyboard
        .as_ref()
        .expect("配置了底部按钮时回复必须带上键盘");
    let raw = keyboard.to_string();
    assert!(
        raw.contains("aurorix.footer."),
        "键盘里应当有底部按钮的 id, 实得 {raw}"
    );
    assert!(raw.contains("帮助"), "键盘里应当有按钮文案");
}

#[tokio::test]
async fn no_footer_reply_skips_footer_buttons() {
    let transport = RecordingTransport::new();
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let registry = empty_registry();
    registry.register(
        "demo",
        CommandSpec::new("/silent").on_sync(|_invocation: &Invocation| {
            Ok(Some(Outgoing::new(ReplyKind::Handler, "pong").no_footer()))
        }),
    );
    let pipeline = pipeline(
        Arc::clone(&transport),
        Arc::clone(&kv),
        hub(&["帮助 | command | /help"]),
        &registry,
    );
    pipeline.handle(&group_message("/silent")).await;
    let sent = transport.sent();
    assert_eq!(sent.len(), 1, "只应发一条回复");
    assert!(
        sent[0].1.keyboard.is_none(),
        "声明了 no_footer 的回复不应被追加底部按钮"
    );
}

#[tokio::test]
async fn counter_survives_a_failing_transport() {
    let transport = RecordingTransport::new();
    transport.failures.store(1, Ordering::SeqCst);
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    let pipeline = pipeline(
        Arc::clone(&transport),
        Arc::clone(&kv),
        hub(&["+1 | counter | ; dedup=none"]),
        &empty_registry(),
    );
    let report = pipeline.handle(&click("0", "u1")).await;
    assert_eq!(
        report.outcome,
        Outcome::SendFailed,
        "发送失败必须如实报告, 不能谎报成功"
    );
    assert!(report.send_error.is_some(), "失败原因要能被上层看到");
}

#[tokio::test]
async fn counters_are_namespaced_per_bucket() {
    let kv: Arc<dyn KvBackend> = Arc::new(MemoryKv::new());
    kv.incr("footer.+1@g1", 1).await.expect("计数写入应当成功");
    let scoped = kv.get("footer.+1@g1").await.ok().flatten();
    let bare = kv.get("footer.+1").await.ok().flatten();
    assert_eq!(scoped, Some(json!(1)), "群内计数应当落在带会话后缀的键上");
    assert!(bare.is_none(), "不带会话后缀的键不应被写入");
}
