use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_api::{ApiError, Outbound, SendOutcome};
use aurorix_bot::{BotConfig, Logger, Outcome, Pipeline, PipelineDeps, ReplyTransport, Stage};
use aurorix_core::Target;
use aurorix_gateway::{BoxFuture, ProtocolEvent};
use aurorix_log::{Level, Options};
use aurorix_plugin::{
    BanEntry, BanList, CallbackBook, CallbackFuture, CallbackRequest, CommandRegistry, CommandSpec,
    Invocation, Outgoing, PluginCommands, Rate, ReplyKind, Router, DEFAULT_BAN_MESSAGE,
};
use serde_json::json;

struct GateTransport {
    sent: Mutex<Vec<Outbound>>,
    acks: Mutex<Vec<String>>,
}

impl GateTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
            acks: Mutex::new(Vec::new()),
        })
    }

    fn contents(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|outbound| outbound.content.clone())
            .collect()
    }

    fn acks(&self) -> Vec<String> {
        self.acks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl ReplyTransport for GateTransport {
    fn send<'a>(
        &'a self,
        target: &'a Target,
        message: &'a Outbound,
    ) -> BoxFuture<'a, Result<SendOutcome, ApiError>> {
        let target = target.clone();
        let message = message.clone();
        Box::pin(async move {
            self.sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(message);
            Ok(SendOutcome {
                message_id: Some(String::from("out-1")),
                audit_id: None,
                target,
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

fn registry() -> Arc<CommandRegistry> {
    let registry = CommandRegistry::new();
    registry.register_plugin(
        PluginCommands::new("demo", "示例插件").command(CommandSpec::new("echo").on(
            |invocation: Invocation| async move {
                Ok(Some(Outgoing::new(ReplyKind::Handler, invocation.text)))
            },
        )),
    );
    Arc::new(registry)
}

fn banned_list() -> Arc<BanList> {
    Arc::new(BanList::from_entries([BanEntry::new("u1")]))
}

fn pipeline_with(
    transport: Arc<GateTransport>,
    router: Router,
    book: Arc<CallbackBook>,
) -> Arc<Pipeline> {
    let deps = PipelineDeps::new(
        Arc::new(BotConfig::new("1000", "secret")),
        Arc::new(router),
        transport,
        logger(),
    )
    .with_callbacks(book);
    Pipeline::new(deps)
}

fn group_event(event_id: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(event_id.to_string()),
        op: 0,
        event_type: Some(String::from("GROUP_AT_MESSAGE_CREATE")),
        seq: Some(2),
        data: json!({
            "id": "msg-1",
            "content": "/echo hi",
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

fn click(event_id: &str, button_id: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(event_id.to_string()),
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
                    "button_data": "pick",
                    "button_id": button_id,
                    "user_id": "u1"
                }
            }
        }),
        raw: String::new(),
    }
}

#[tokio::test]
async fn banned_user_message_is_refused() {
    let transport = GateTransport::new();
    let router = Router::new(registry()).with_ban(banned_list());
    let pipeline = pipeline_with(Arc::clone(&transport), router, CallbackBook::new());
    let report = pipeline.handle(&group_event("evt-ban")).await;
    assert_eq!(report.stage, Some(Stage::Banned), "消息路径必须命中封禁");
    assert!(report.sent, "封禁提示要发出去");
    let contents = transport.contents();
    assert_eq!(contents.len(), 1);
    assert!(
        contents[0].contains(DEFAULT_BAN_MESSAGE),
        "封禁文案要按配置下发: {contents:?}"
    );
}

#[tokio::test]
async fn banned_user_interaction_is_refused() {
    let transport = GateTransport::new();
    let router = Router::new(registry()).with_ban(banned_list());
    let pipeline = pipeline_with(Arc::clone(&transport), router, CallbackBook::new());
    let report = pipeline.handle(&click("evt-int-ban", "demo.pick")).await;
    assert_eq!(report.stage, Some(Stage::Banned), "交互路径必须命中封禁");
    assert_eq!(report.outcome, Outcome::Replied);
    assert_eq!(transport.acks(), vec![String::from("int-1")], "封禁也要先回执");
    let contents = transport.contents();
    assert_eq!(contents.len(), 1);
    assert!(
        contents[0].contains(DEFAULT_BAN_MESSAGE),
        "交互封禁文案要下发: {contents:?}"
    );
}

#[tokio::test]
async fn bound_button_click_consumes_the_command_bucket() {
    let transport = GateTransport::new();
    let router = Router::new(registry());
    router
        .limiter()
        .bind_click("demo.pick", "demo.pick", Rate::per_window(1, 60));
    let book = CallbackBook::new();
    book.register_scoped(
        "demo.pick",
        "demo",
        Arc::new(|_request: CallbackRequest| {
            let future: CallbackFuture =
                Box::pin(async move { Some(Outgoing::new(ReplyKind::Handler, "点到了")) });
            future
        }),
    );
    let pipeline = pipeline_with(Arc::clone(&transport), router, book);
    let first = pipeline.handle(&click("evt-click-1", "demo.pick")).await;
    assert_eq!(first.outcome, Outcome::Replied);
    let second = pipeline.handle(&click("evt-click-2", "demo.pick")).await;
    assert_eq!(second.outcome, Outcome::Replied);
    let contents = transport.contents();
    assert_eq!(contents.len(), 2);
    assert_eq!(contents[0], "点到了");
    assert!(
        contents[1].contains("触发太频繁"),
        "超出额度要回限流提示: {contents:?}"
    );
}

#[tokio::test]
async fn unbound_button_click_is_not_limited() {
    let transport = GateTransport::new();
    let router = Router::new(registry());
    let book = CallbackBook::new();
    book.register_scoped(
        "demo.free",
        "demo",
        Arc::new(|_request: CallbackRequest| {
            let future: CallbackFuture =
                Box::pin(async move { Some(Outgoing::new(ReplyKind::Handler, "点到了")) });
            future
        }),
    );
    let pipeline = pipeline_with(Arc::clone(&transport), router, book);
    for index in 1..=3 {
        let report = pipeline
            .handle(&click(&format!("evt-free-{index}"), "demo.free"))
            .await;
        assert_eq!(report.outcome, Outcome::Replied);
    }
    assert_eq!(
        transport.contents(),
        vec![
            String::from("点到了"),
            String::from("点到了"),
            String::from("点到了")
        ],
        "没绑定限流的按钮不该被拦"
    );
}

#[tokio::test]
async fn ban_throttle_keeps_the_window() {
    let list = banned_list();
    assert!(list.hit("u1").is_some(), "第一次要拦");
    assert!(
        list.hit("u1").is_none(),
        "封禁提示在 {:?} 内只回一次",
        Duration::from_secs(15)
    );
    assert!(list.hit("u2").is_none(), "不在名单里就不拦");
}
