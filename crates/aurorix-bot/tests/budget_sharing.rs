use std::sync::{Arc, Mutex};

use aurorix_api::{ApiError, Outbound, SendOutcome};
use aurorix_bot::{BotConfig, Logger, Pipeline, PipelineDeps, ReplyTransport, Stage};
use aurorix_core::Target;
use aurorix_gateway::{BoxFuture, ProtocolEvent};
use aurorix_log::{Level, Options};
use aurorix_plugin::{
    CommandRegistry, CommandSpec, Invocation, Outgoing, PluginCommands, ReplyKind, Router,
};
use serde_json::json;

struct SeqTransport {
    sent: Mutex<Vec<(Option<u32>, bool)>>,
}

impl SeqTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
        })
    }

    fn seqs(&self) -> Vec<Option<u32>> {
        self.sent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(seq, _)| *seq)
            .collect()
    }

    fn active(&self) -> Vec<bool> {
        self.sent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(_, active)| *active)
            .collect()
    }
}

impl ReplyTransport for SeqTransport {
    fn send<'a>(
        &'a self,
        target: &'a Target,
        message: &'a Outbound,
    ) -> BoxFuture<'a, Result<SendOutcome, ApiError>> {
        let seq = match (message.anchor.empty(), &message.budget) {
            (false, Some(budget)) => budget.count().ok(),
            _ => None,
        };
        let active = seq.is_none();
        let outcome = SendOutcome {
            message_id: Some(format!("out-{}", self.seqs().len() + 1)),
            audit_id: None,
            target: target.clone(),
            delivered_active: active,
            fallback: false,
        };
        self.sent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((seq, active));
        Box::pin(async move { Ok(outcome) })
    }
}

fn logger() -> Arc<Logger> {
    Logger::new(Options::default().with_level(Level::Error))
}

fn registry() -> Arc<CommandRegistry> {
    let registry = CommandRegistry::new();
    registry.register_plugin(
        PluginCommands::new("demo", "示例插件").command(CommandSpec::new("burst").on(
            |invocation: Invocation| async move {
                for index in 1..=6 {
                    invocation
                        .send(Outgoing::new(
                            ReplyKind::Handler,
                            format!("第 {index} 条"),
                        ))
                        .await?;
                }
                Ok(None)
            },
        )),
    );
    Arc::new(registry)
}

fn pipeline(transport: Arc<SeqTransport>) -> Arc<Pipeline> {
    let deps = PipelineDeps::new(
        Arc::new(BotConfig::new("1000", "secret")),
        Arc::new(Router::new(registry())),
        transport,
        logger(),
    );
    Pipeline::new(deps)
}

fn group_event(message_id: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(format!("evt-{message_id}")),
        op: 0,
        event_type: Some(String::from("GROUP_AT_MESSAGE_CREATE")),
        seq: Some(2),
        data: json!({
            "id": message_id,
            "content": "/burst",
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
async fn one_sender_shares_a_single_seq_budget() {
    let transport = SeqTransport::new();
    let pipeline = pipeline(Arc::clone(&transport));
    let report = pipeline.handle(&group_event("m-burst")).await;
    assert_eq!(report.stage, Some(Stage::Handled));
    assert_eq!(
        transport.seqs(),
        vec![Some(1), Some(2), Some(3), Some(4), Some(5), None],
        "同一个 InvocationSender 连推 6 条, msg_seq 必须依次递增, 第六条转主动"
    );
    assert_eq!(
        transport.active(),
        vec![false, false, false, false, false, true],
        "只有预算用光后的那条走主动通道"
    );
}

#[tokio::test]
async fn a_second_event_gets_its_own_budget() {
    let transport = SeqTransport::new();
    let pipeline = pipeline(Arc::clone(&transport));
    pipeline.handle(&group_event("m-first")).await;
    pipeline.handle(&group_event("m-second")).await;
    let seqs = transport.seqs();
    assert_eq!(
        seqs,
        vec![
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            None,
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            None
        ],
        "换一条入站消息必须换一份预算"
    );
}

#[tokio::test]
async fn budgets_are_cached_per_message_id() {
    let transport = SeqTransport::new();
    let pipeline = pipeline(Arc::clone(&transport));
    let event = group_event("m-cache");
    let message = aurorix_bot::MessageEvent::parse(&event).expect("事件要能解析");
    let first = pipeline.budget_for(&message);
    let second = pipeline.budget_for(&message);
    assert!(
        Arc::ptr_eq(&first, &second),
        "同一条入站消息必须拿到同一份预算"
    );
    let other = pipeline.budget_of("m-other");
    assert!(
        !Arc::ptr_eq(&first, &other),
        "不同消息不能共用预算"
    );
}
