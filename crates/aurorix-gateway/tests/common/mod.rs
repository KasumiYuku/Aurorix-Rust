#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use aurorix_gateway::{
    BoxFuture, ConnectRequest, EventSink, Frame, FrameIo, GatewayClient, GatewayConfig,
    GatewayState, ProtocolEvent, StaticTokenSource, Transport,
};
use serde_json::Value;
use tokio::sync::Notify;

pub fn lock<T>(target: &Mutex<T>) -> MutexGuard<'_, T> {
    match target.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn text(value: serde_json::Value) -> Frame {
    Frame::Text(value.to_string())
}

pub fn hello(interval_ms: u64) -> Frame {
    text(serde_json::json!({"op": 10, "d": {"heartbeat_interval": interval_ms}}))
}

pub fn ack() -> Frame {
    text(serde_json::json!({"op": 11}))
}

pub fn ready(session: &str, bot_id: &str, name: &str, seq: i64) -> Frame {
    text(serde_json::json!({
        "op": 0,
        "s": seq,
        "t": "READY",
        "d": {"session_id": session, "user": {"id": bot_id, "username": name}},
    }))
}

pub fn resumed(seq: i64) -> Frame {
    text(serde_json::json!({"op": 0, "s": seq, "t": "RESUMED", "d": {}}))
}

pub fn dispatch(event: &str, seq: i64, data: serde_json::Value) -> Frame {
    text(serde_json::json!({
        "op": 0,
        "s": seq,
        "t": event,
        "id": format!("evt-{seq}"),
        "d": data,
    }))
}

pub fn group_message(seq: i64, content: &str) -> Frame {
    dispatch(
        "GROUP_AT_MESSAGE_CREATE",
        seq,
        serde_json::json!({
            "id": format!("m-{seq}"),
            "content": content,
            "group_openid": "g-1",
            "author": {"member_openid": "u-1"},
        }),
    )
}

#[derive(Debug, Clone, Default)]
pub struct ConnScript {
    pub prelude: Vec<Frame>,
    pub on_identify: Vec<Frame>,
    pub on_resume: Vec<Frame>,
    pub on_heartbeat: Vec<Frame>,
}

impl ConnScript {
    pub fn hello(interval_ms: u64) -> Self {
        Self {
            prelude: vec![hello(interval_ms)],
            ..Self::default()
        }
    }

    pub fn with_identify(mut self, frames: Vec<Frame>) -> Self {
        self.on_identify = frames;
        self
    }

    pub fn with_resume(mut self, frames: Vec<Frame>) -> Self {
        self.on_resume = frames;
        self
    }

    pub fn with_heartbeat(mut self, frames: Vec<Frame>) -> Self {
        self.on_heartbeat = frames;
        self
    }
}

struct IoState {
    inbound: VecDeque<Frame>,
    sent: Vec<Frame>,
    closed: bool,
}

pub struct FakeIo {
    state: Mutex<IoState>,
    notify: Notify,
    script: ConnScript,
}

impl FakeIo {
    pub fn new(script: ConnScript) -> Self {
        let mut inbound = VecDeque::new();
        for frame in &script.prelude {
            inbound.push_back(frame.clone());
        }
        Self {
            state: Mutex::new(IoState {
                inbound,
                sent: Vec::new(),
                closed: false,
            }),
            notify: Notify::new(),
            script,
        }
    }

    pub fn sent(&self) -> Vec<Frame> {
        lock(&self.state).sent.clone()
    }

    pub fn sent_json(&self) -> Vec<Value> {
        self.sent()
            .iter()
            .filter_map(Frame::as_text)
            .filter_map(|text| serde_json::from_str::<Value>(text).ok())
            .collect()
    }

    pub fn is_closed(&self) -> bool {
        lock(&self.state).closed
    }

    pub fn push(&self, frame: Frame) {
        lock(&self.state).inbound.push_back(frame);
        self.notify.notify_waiters();
    }

    fn react(&self, frame: &Frame) {
        let Some(text) = frame.as_text() else {
            return;
        };
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let op = value.get("op").and_then(Value::as_i64).unwrap_or(-1);
        let reply = match op {
            2 => &self.script.on_identify,
            6 => &self.script.on_resume,
            1 => &self.script.on_heartbeat,
            _ => return,
        };
        if reply.is_empty() {
            return;
        }
        let mut state = lock(&self.state);
        for item in reply {
            state.inbound.push_back(item.clone());
        }
        drop(state);
        self.notify.notify_waiters();
    }
}

impl FrameIo for FakeIo {
    fn recv(&self) -> BoxFuture<'_, Result<Frame, aurorix_gateway::GatewayError>> {
        Box::pin(async move {
            loop {
                {
                    let mut state = lock(&self.state);
                    if let Some(frame) = state.inbound.pop_front() {
                        return Ok(frame);
                    }
                    if state.closed {
                        return Ok(Frame::Close(1006));
                    }
                }
                self.notify.notified().await;
            }
        })
    }

    fn send(&self, frame: Frame) -> BoxFuture<'_, Result<(), aurorix_gateway::GatewayError>> {
        Box::pin(async move {
            lock(&self.state).sent.push(frame.clone());
            self.react(&frame);
            Ok(())
        })
    }

    fn close(&self, _code: u16) -> BoxFuture<'_, Result<(), aurorix_gateway::GatewayError>> {
        Box::pin(async move {
            lock(&self.state).closed = true;
            self.notify.notify_waiters();
            Ok(())
        })
    }
}

pub struct CountingTransport {
    connects: AtomicU64,
    scripts: Box<dyn Fn(u64) -> ConnScript + Send + Sync>,
    sockets: Mutex<Vec<Arc<FakeIo>>>,
}

impl CountingTransport {
    pub fn new(scripts: impl Fn(u64) -> ConnScript + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            connects: AtomicU64::new(0),
            scripts: Box::new(scripts),
            sockets: Mutex::new(Vec::new()),
        })
    }

    pub fn fixed(script: ConnScript) -> Arc<Self> {
        Self::new(move |_| script.clone())
    }

    pub fn connects(&self) -> u64 {
        self.connects.load(Ordering::SeqCst)
    }

    pub fn socket(&self, index: usize) -> Arc<FakeIo> {
        let sockets = lock(&self.sockets);
        Arc::clone(&sockets[index])
    }

    pub fn first_ops(&self) -> Vec<i64> {
        let sockets = lock(&self.sockets);
        sockets
            .iter()
            .map(|socket| {
                socket
                    .sent_json()
                    .first()
                    .and_then(|value| value.get("op").and_then(Value::as_i64))
                    .unwrap_or(-1)
            })
            .collect()
    }
}

impl Transport for CountingTransport {
    fn connect<'a>(
        &'a self,
        _request: ConnectRequest,
    ) -> BoxFuture<'a, Result<Arc<dyn FrameIo>, aurorix_gateway::GatewayError>> {
        Box::pin(async move {
            let index = self.connects.fetch_add(1, Ordering::SeqCst);
            let script = (self.scripts)(index);
            let socket = Arc::new(FakeIo::new(script));
            lock(&self.sockets).push(Arc::clone(&socket));
            let io: Arc<dyn FrameIo> = socket;
            Ok(io)
        })
    }
}

pub struct RecordingSink {
    events: Mutex<Vec<ProtocolEvent>>,
    notify: Notify,
}

impl RecordingSink {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(Vec::new()),
            notify: Notify::new(),
        })
    }

    pub fn events(&self) -> Vec<ProtocolEvent> {
        lock(&self.events).clone()
    }

    pub fn len(&self) -> usize {
        lock(&self.events).len()
    }

    pub async fn wait_for(&self, count: usize, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.len() >= count {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            let _ = tokio::time::timeout(Duration::from_millis(5), self.notify.notified()).await;
        }
    }
}

impl EventSink for RecordingSink {
    fn dispatch(&self, event: ProtocolEvent) {
        lock(&self.events).push(event);
        self.notify.notify_waiters();
    }
}

pub fn fast_config(url: &str) -> GatewayConfig {
    GatewayConfig::new(url, aurorix_gateway::DEFAULT_INTENT_BITS)
        .with_backoff(Duration::from_millis(10), Duration::from_millis(40))
        .with_reauth_delay(Duration::from_millis(10))
        .with_read_window(Duration::from_millis(400))
}

pub fn build(
    config: GatewayConfig,
    token: &str,
    transport: Arc<CountingTransport>,
) -> (
    Arc<GatewayClient>,
    Arc<StaticTokenSource>,
    Arc<RecordingSink>,
) {
    let tokens = Arc::new(StaticTokenSource::new(token));
    let sink = RecordingSink::new();
    let client = Arc::new(GatewayClient::with_transport(
        config,
        tokens.clone(),
        sink.clone(),
        transport,
    ));
    (client, tokens, sink)
}

pub async fn wait_state(client: &GatewayClient, state: GatewayState, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if client.state() == state {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

pub async fn wait_until(mut check: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if check() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}
