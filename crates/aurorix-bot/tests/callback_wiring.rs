use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use aurorix_api::{ApiError, Outbound, SendOutcome};
use aurorix_bot::{BotConfig, Logger, Outcome, Pipeline, PipelineDeps, ReplyTransport};
use aurorix_core::Target;
use aurorix_gateway::{BoxFuture, ProtocolEvent};
use aurorix_log::{Level, Options};
use aurorix_plugin::{
    Attachment, CallbackBook, CallbackFuture, CallbackRequest, CommandRegistry, Outgoing,
    ReplyKind, Router,
};
use serde_json::json;

struct RecordingTransport {
    sent: Mutex<Vec<(Target, Outbound)>>,
    acks: Mutex<Vec<String>>,
    seqs: Mutex<Vec<Option<u32>>>,
    images: Mutex<Vec<(String, String)>>,
}

impl RecordingTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
            acks: Mutex::new(Vec::new()),
            seqs: Mutex::new(Vec::new()),
            images: Mutex::new(Vec::new()),
        })
    }

    fn contents(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(_, outbound)| outbound.content.clone())
            .collect()
    }

    fn acks(&self) -> Vec<String> {
        self.acks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn seqs(&self) -> Vec<Option<u32>> {
        self.seqs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn images(&self) -> Vec<(String, String)> {
        self.images
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
        let seq = match (message.anchor.empty(), &message.budget) {
            (false, Some(budget)) => budget.count().ok(),
            _ => None,
        };
        Box::pin(async move {
            let index = self
                .sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len();
            self.seqs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(seq);
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

    fn send_image_bytes<'a>(
        &'a self,
        _target: &'a Target,
        content: String,
        _data: Vec<u8>,
        filename: String,
        _mime_type: String,
        _msg_id: Option<String>,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        Box::pin(async move {
            self.images
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((content, filename));
            Ok(Some(String::from("img-1")))
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

fn pipeline(transport: Arc<RecordingTransport>, book: Arc<CallbackBook>) -> Arc<Pipeline> {
    let deps = PipelineDeps::new(
        Arc::new(BotConfig::new("1000", "secret")),
        Arc::new(Router::new(Arc::new(CommandRegistry::new()))),
        transport,
        logger(),
    )
    .with_callbacks(book);
    Pipeline::new(deps)
}

fn click(button_id: &str, button_data: &str) -> ProtocolEvent {
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
                    "button_id": button_id,
                    "user_id": "u1"
                }
            }
        }),
        raw: String::new(),
    }
}

#[tokio::test]
async fn plugin_callback_button_reaches_the_book_and_replies() {
    let transport = RecordingTransport::new();
    let book = CallbackBook::new();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let registered = book.register_scoped(
        "arcaea.score.best50",
        "arcaea",
        Arc::new(move |request: CallbackRequest| {
            let counter = Arc::clone(&counter);
            let future: CallbackFuture = Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Some(Outgoing::new(
                    ReplyKind::Handler,
                    format!("{} · {} · {}", request.plugin, request.data, request.user),
                ))
            });
            future
        }),
    );
    assert!(registered, "首次注册必须成功");
    let pipeline = pipeline(Arc::clone(&transport), book);
    let report = pipeline.handle(&click("arcaea.score.best50", "50")).await;
    assert_eq!(report.outcome, Outcome::Replied, "回调按钮应当产生回复");
    assert!(report.sent, "回复必须真的发出去");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "handler 必须被调用一次");
    assert_eq!(transport.contents(), vec![String::from("arcaea · 50 · u1")]);
    assert_eq!(transport.acks(), vec![String::from("int-1")]);
}

#[tokio::test]
async fn unknown_callback_button_stays_silent() {
    let transport = RecordingTransport::new();
    let book = CallbackBook::new();
    let pipeline = pipeline(Arc::clone(&transport), book);
    let report = pipeline.handle(&click("arcaea.absent", "x")).await;
    assert_eq!(report.outcome, Outcome::Interaction, "没有 handler 就不该回复");
    assert!(!report.sent);
    assert!(transport.contents().is_empty());
    assert_eq!(transport.acks(), vec![String::from("int-1")]);
}

fn channel_click(button_id: &str) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(String::from("ev-int-channel")),
        op: 0,
        event_type: Some(String::from("INTERACTION_CREATE")),
        seq: Some(8),
        data: json!({
            "id": "int-2",
            "type": 11,
            "scene": "guild",
            "chat_type": 2,
            "channel_id": "c1",
            "guild_id": "gd1",
            "data": {
                "type": 11,
                "resolved": {
                    "button_data": "card",
                    "button_id": button_id,
                    "user_id": "u1"
                }
            }
        }),
        raw: String::new(),
    }
}

#[tokio::test]
async fn callback_request_sender_shares_the_interaction_budget() {
    let transport = RecordingTransport::new();
    let book = CallbackBook::new();
    book.register_scoped(
        "arcaea.score.recent",
        "arcaea",
        Arc::new(|request: CallbackRequest| {
            let future: CallbackFuture = Box::pin(async move {
                let sender = request.sender.expect("回调必须拿到 sender");
                for index in 1..=2 {
                    sender
                        .send(Outgoing::new(ReplyKind::Handler, format!("中途 {index}")))
                        .await
                        .ok()?;
                }
                Some(Outgoing::new(
                    ReplyKind::Handler,
                    format!("回执 {}", request.interaction_id),
                ))
            });
            future
        }),
    );
    let pipeline = pipeline(Arc::clone(&transport), book);
    let report = pipeline.handle(&click("arcaea.score.recent", "5")).await;
    assert_eq!(report.outcome, Outcome::Replied, "{:?}", report);
    assert_eq!(
        transport.contents(),
        vec![
            String::from("中途 1"),
            String::from("中途 2"),
            String::from("回执 int-1")
        ],
        "回调里能连续推消息, interaction_id 要带过来"
    );
    assert_eq!(
        transport.seqs(),
        vec![Some(1), Some(2), Some(3)],
        "回调 sender 必须与交互回复共用同一份 msg_seq 预算"
    );
}

#[tokio::test]
async fn callback_media_goes_through_the_channel_image_path() {
    let transport = RecordingTransport::new();
    let book = CallbackBook::new();
    book.register_scoped(
        "arcaea.score.card",
        "arcaea",
        Arc::new(|_request: CallbackRequest| {
            let future: CallbackFuture = Box::pin(async move {
                Some(
                    Outgoing::new(ReplyKind::Handler, "成绩卡")
                        .image(Attachment::new(vec![1u8, 2, 3, 4], "card.png", "image/png")),
                )
            });
            future
        }),
    );
    let pipeline = pipeline(Arc::clone(&transport), book);
    let report = pipeline.handle(&channel_click("arcaea.score.card")).await;
    assert_eq!(report.outcome, Outcome::Replied, "{:?}", report);
    assert!(report.rendered, "回调媒体要和命令回复走同一条投递路径");
    assert_eq!(
        transport.images(),
        vec![(String::from("成绩卡"), String::from("card.png"))],
        "回调媒体必须保留正文 caption"
    );
    assert!(transport.contents().is_empty(), "有附件就不再发纯文本");
}
