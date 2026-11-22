use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use aurorix_log::Logger;
use chrono::{DateTime, Utc};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::backoff::{jitter, Backoff};
use crate::codes::{classify, CloseAction};
use crate::error::GatewayError;
use crate::frame::{
    heartbeat_frame, identify_frame, parse_hello, parse_ready, resume_frame, ProtocolEvent,
    OP_DISPATCH, OP_HEARTBEAT, OP_HEARTBEAT_ACK, OP_HELLO, OP_INVALID_SESSION, OP_RECONNECT,
};
use crate::intents::DEFAULT_INTENT_BITS;
use crate::source::{EventSink, GatewaySource, TokenSource};
use crate::state::{GatewayState, GatewayStats};
use crate::transport::{ConnectRequest, Frame, FrameIo, Transport, WebSocketTransport};

pub const DEFAULT_GATEWAY_URL: &str = "wss://api.sgroup.qq.com/websocket";

pub const GATEWAY_MODULE: &str = "gateway";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayConfig {
    pub url: String,
    pub intents: u64,
    pub shard: [u32; 2],
    pub headers: Vec<(String, String)>,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    pub read_window: Duration,
    pub base_backoff: Duration,
    pub max_backoff: Duration,
    pub max_strikes: u32,
    pub reauth_delay: Duration,
    pub reconnect_grace: Duration,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            url: DEFAULT_GATEWAY_URL.to_string(),
            intents: DEFAULT_INTENT_BITS,
            shard: [0, 1],
            headers: Vec::new(),
            connect_timeout: Duration::from_millis(15_000),
            handshake_timeout: Duration::from_millis(15_000),
            read_window: Duration::from_millis(90_000),
            base_backoff: Duration::from_millis(1_000),
            max_backoff: Duration::from_millis(30_000),
            max_strikes: 5,
            reauth_delay: Duration::from_millis(3_000),
            reconnect_grace: Duration::from_millis(1_000),
        }
    }
}

impl GatewayConfig {
    pub fn new(url: impl Into<String>, intents: u64) -> Self {
        Self {
            url: url.into(),
            intents,
            ..Self::default()
        }
    }

    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_backoff(mut self, base: Duration, max: Duration) -> Self {
        self.base_backoff = base;
        self.max_backoff = max;
        self
    }

    pub fn with_read_window(mut self, window: Duration) -> Self {
        self.read_window = window;
        self
    }

    pub fn with_reauth_delay(mut self, delay: Duration) -> Self {
        self.reauth_delay = delay;
        self
    }

    pub fn with_strikes(mut self, strikes: u32) -> Self {
        self.max_strikes = strikes;
        self
    }

    pub fn with_shard(mut self, shard: [u32; 2]) -> Self {
        self.shard = shard;
        self
    }
}

#[derive(Debug)]
struct Inner {
    state: GatewayState,
    shard: [u32; 2],
    session_id: Option<String>,
    bot_id: Option<String>,
    bot_name: Option<String>,
    seq: i64,
    ready_count: u64,
    reconnects: u64,
    last_close_code: Option<u16>,
    last_event_at: Option<DateTime<Utc>>,
    heartbeat_interval_ms: Option<u64>,
    connected_at: Option<Instant>,
    live_tasks: usize,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            state: GatewayState::Disconnected,
            shard: [0, 1],
            session_id: None,
            bot_id: None,
            bot_name: None,
            seq: 0,
            ready_count: 0,
            reconnects: 0,
            last_close_code: None,
            last_event_at: None,
            heartbeat_interval_ms: None,
            connected_at: None,
            live_tasks: 0,
        }
    }
}

pub struct GatewayClient {
    config: GatewayConfig,
    tokens: Arc<dyn TokenSource>,
    sink: Arc<dyn EventSink>,
    transport: Arc<dyn Transport>,
    source: Option<Arc<dyn GatewaySource>>,
    inner: Mutex<Inner>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    task: Mutex<Option<JoinHandle<()>>>,
    logger: Option<Arc<Logger>>,
}

fn lock<T>(target: &Mutex<T>) -> MutexGuard<'_, T> {
    match target.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn short_session(session: &str) -> String {
    let head: String = session.chars().take(8).collect();
    if session.chars().count() > 8 {
        format!("{head}…")
    } else {
        head
    }
}

impl GatewayClient {
    pub fn new(
        config: GatewayConfig,
        tokens: Arc<dyn TokenSource>,
        sink: Arc<dyn EventSink>,
    ) -> Self {
        Self::with_transport(config, tokens, sink, Arc::new(WebSocketTransport::new()))
    }

    pub fn with_transport(
        config: GatewayConfig,
        tokens: Arc<dyn TokenSource>,
        sink: Arc<dyn EventSink>,
        transport: Arc<dyn Transport>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let inner = Inner {
            shard: config.shard,
            ..Inner::default()
        };
        Self {
            config,
            tokens,
            sink,
            transport,
            source: None,
            inner: Mutex::new(inner),
            shutdown_tx,
            shutdown_rx,
            task: Mutex::new(None),
            logger: None,
        }
    }

    pub fn with_source(mut self, source: Arc<dyn GatewaySource>) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_logger(mut self, logger: Arc<Logger>) -> Self {
        self.logger = Some(logger);
        self
    }

    pub fn info(&self, message: &str) {
        if let Some(logger) = &self.logger {
            logger.info(GATEWAY_MODULE, message);
        }
    }

    pub fn warn(&self, message: &str) {
        if let Some(logger) = &self.logger {
            logger.warn(GATEWAY_MODULE, message);
        }
    }

    pub fn config(&self) -> &GatewayConfig {
        &self.config
    }

    pub fn state(&self) -> GatewayState {
        lock(&self.inner).state
    }

    pub fn stats(&self) -> GatewayStats {
        let inner = lock(&self.inner);
        let connected_since_ms = match inner.connected_at {
            Some(at) => at.elapsed().as_millis() as i64,
            None => 0,
        };
        GatewayStats {
            state: inner.state,
            reconnects: inner.reconnects,
            last_close_code: inner.last_close_code,
            last_event_at: inner.last_event_at,
            seq: inner.seq,
            ready: inner.state.is_ready(),
            session_id: inner.session_id.clone(),
            bot_id: inner.bot_id.clone(),
            bot_name: inner.bot_name.clone(),
            shard: inner.shard,
            heartbeat_interval_ms: inner.heartbeat_interval_ms,
            connected_since_ms,
            live_tasks: inner.live_tasks,
        }
    }

    pub fn live_tasks(&self) -> usize {
        lock(&self.inner).live_tasks
    }

    pub fn is_stopped(&self) -> bool {
        self.state().is_terminal()
    }

    pub fn is_shutdown(&self) -> bool {
        *self.shutdown_rx.borrow()
    }

    pub fn session_id(&self) -> Option<String> {
        lock(&self.inner).session_id.clone()
    }

    pub fn seq(&self) -> i64 {
        lock(&self.inner).seq
    }

    pub fn shard(&self) -> [u32; 2] {
        lock(&self.inner).shard
    }

    fn set_shard(&self, shard: [u32; 2]) {
        lock(&self.inner).shard = shard;
    }

    pub fn ready_count(&self) -> u64 {
        lock(&self.inner).ready_count
    }

    pub fn start(self: &Arc<Self>) {
        {
            let mut slot = lock(&self.task);
            if slot.is_some() {
                return;
            }
            let this = Arc::clone(self);
            *slot = Some(tokio::spawn(async move {
                this.run().await;
            }));
        }
    }

    pub async fn shutdown(self: &Arc<Self>) {
        let _ = self.shutdown_tx.send(true);
        let handle = { lock(&self.task).take() };
        if let Some(handle) = handle {
            let _ = handle.await;
        }
    }

    fn set_state(&self, state: GatewayState) {
        let previous = {
            let mut inner = lock(&self.inner);
            let previous = inner.state;
            inner.state = state;
            previous
        };
        if previous != state {
            self.info(&format!(
                "网关状态 {} → {}",
                previous.as_str(),
                state.as_str()
            ));
        }
    }

    fn add_live_task(&self) {
        let mut inner = lock(&self.inner);
        inner.live_tasks += 1;
    }

    fn remove_live_task(&self) {
        let mut inner = lock(&self.inner);
        if inner.live_tasks > 0 {
            inner.live_tasks -= 1;
        }
    }

    fn bump_seq(&self, seq: Option<i64>) {
        let mut inner = lock(&self.inner);
        if let Some(value) = seq {
            if value > inner.seq {
                inner.seq = value;
            }
        }
    }

    fn drop_session(&self) {
        let mut inner = lock(&self.inner);
        inner.session_id = None;
        inner.seq = 0;
    }

    fn note_close(&self, code: Option<u16>) {
        if let Some(code) = code {
            lock(&self.inner).last_close_code = Some(code);
        }
    }

    fn note_reconnect(&self) {
        lock(&self.inner).reconnects += 1;
    }

    fn note_event(&self) {
        lock(&self.inner).last_event_at = Some(Utc::now());
    }

    fn on_connected(&self) {
        let mut inner = lock(&self.inner);
        inner.connected_at = Some(Instant::now());
    }

    fn set_heartbeat_interval(&self, interval_ms: u64) {
        lock(&self.inner).heartbeat_interval_ms = Some(interval_ms);
    }

    fn apply_ready(&self, raw: &str) {
        let info = parse_ready(raw);
        let session = short_session(&info.session_id);
        let bot = if info.bot_name.is_empty() {
            info.bot_id.clone()
        } else {
            info.bot_name.clone()
        };
        {
            let mut inner = lock(&self.inner);
            inner.session_id = Some(info.session_id);
            inner.bot_id = Some(info.bot_id);
            inner.bot_name = Some(info.bot_name);
            inner.ready_count += 1;
            inner.connected_at = Some(Instant::now());
        }
        self.set_state(GatewayState::Ready);
        self.info(&format!(
            "收到 READY bot={bot} shard=[{}, {}] session={session}",
            self.shard()[0],
            self.shard()[1]
        ));
    }

    fn apply_resumed(&self) {
        {
            let mut inner = lock(&self.inner);
            inner.ready_count += 1;
        }
        self.set_state(GatewayState::Ready);
    }

    fn count_strike(&self, strikes: u32, before: u64) -> u32 {
        if self.ready_count() != before {
            return 0;
        }
        let next = strikes + 1;
        if next >= self.config.max_strikes {
            self.drop_session();
            return 0;
        }
        next
    }

    async fn sleep(&self, delay: Duration) -> bool {
        if delay.is_zero() {
            return !self.is_shutdown();
        }
        let mut shutdown = self.shutdown_rx.clone();
        tokio::select! {
            _ = tokio::time::sleep(delay) => !self.is_shutdown(),
            _ = shutdown.changed() => false,
        }
    }

    async fn run(self: Arc<Self>) {
        let mut backoff = Backoff::new(self.config.base_backoff, self.config.max_backoff);
        let mut strikes: u32 = 0;
        loop {
            if self.is_shutdown() {
                break;
            }
            let before = self.ready_count();
            let outcome = self.run_session().await;
            if self.is_shutdown() {
                break;
            }
            if self.ready_count() != before {
                backoff.reset();
                strikes = 0;
            }
            match outcome {
                Ok(()) => {
                    backoff.reset();
                    self.set_state(GatewayState::Disconnected);
                    self.info("网关连接正常关闭, 准备重连");
                    if !self.sleep(jitter(self.config.reconnect_grace)).await {
                        break;
                    }
                }
                Err(err) if err.is_shutdown() => break,
                Err(err) if err.is_stop() => {
                    self.note_close(err.close_code());
                    self.set_state(GatewayState::Stopped);
                    self.warn(&format!("网关被服务端关闭, 不再重连: {err}"));
                    return;
                }
                Err(GatewayError::Reconnect) => {
                    backoff.reset();
                    strikes = self.count_strike(strikes, before);
                    self.set_state(GatewayState::Disconnected);
                    self.info("服务端要求重连 (op=7)");
                    if !self.sleep(jitter(self.config.reconnect_grace)).await {
                        break;
                    }
                }
                Err(GatewayError::InvalidSession) => {
                    strikes = self.count_strike(strikes, before);
                    self.drop_session();
                    self.set_state(GatewayState::Disconnected);
                    let delay = self.reauth_delay().await;
                    self.warn(&format!(
                        "会话失效 (op=9), 丢弃会话并重新鉴权, {}ms 后重试",
                        delay.as_millis()
                    ));
                    if !self.sleep(delay).await {
                        break;
                    }
                }
                Err(err) => {
                    strikes = self.count_strike(strikes, before);
                    self.note_close(err.close_code());
                    self.set_state(GatewayState::Disconnected);
                    let delay = jitter(backoff.advance());
                    self.warn(&format!("网关断开: {err}, {}ms 后重试", delay.as_millis()));
                    if !self.sleep(delay).await {
                        break;
                    }
                }
            }
            self.note_reconnect();
        }
        self.info("网关已停止");
        self.set_state(GatewayState::Stopped);
    }

    async fn resolve_endpoint(&self) -> Result<(String, [u32; 2]), GatewayError> {
        let Some(source) = &self.source else {
            return Ok((self.config.url.clone(), self.config.shard));
        };
        let endpoint = source.endpoint().await?;
        let shard = endpoint.shard();
        Ok((endpoint.url, shard))
    }

    async fn reauth_delay(&self) -> Duration {
        match &self.source {
            Some(source) => source.reauth_delay().await,
            None => self.config.reauth_delay,
        }
    }

    async fn run_session(self: &Arc<Self>) -> Result<(), GatewayError> {
        self.set_state(GatewayState::Connecting);
        let (url, shard) = self.resolve_endpoint().await?;
        self.set_shard(shard);
        self.info(&format!("正在换取 access_token url={url}"));
        let token = self.tokens.token().await?;
        self.info("access_token 已就绪, 开始建立 WebSocket 连接");
        let request = ConnectRequest {
            url,
            headers: self.config.headers.clone(),
        };
        let io = match tokio::time::timeout(
            self.config.connect_timeout,
            self.transport.connect(request),
        )
        .await
        {
            Ok(Ok(io)) => io,
            Ok(Err(err)) => return Err(err),
            Err(_) => {
                return Err(GatewayError::Transport(
                    "连接网关超时, 握手未完成".to_string(),
                ))
            }
        };
        self.on_connected();
        self.info("WebSocket 已连接, 等待 HELLO");
        let interval_ms = match self.await_hello(Arc::clone(&io)).await {
            Ok(interval) => interval,
            Err(err) => {
                self.close_quiet(&io).await;
                return Err(err);
            }
        };
        self.set_heartbeat_interval(interval_ms);
        let session = self.session_id();
        let frame = match session {
            Some(session) => {
                self.set_state(GatewayState::Resuming);
                self.info(&format!(
                    "发送 RESUME session={} seq={}",
                    short_session(&session),
                    self.seq()
                ));
                resume_frame(&token, &session, self.seq(), shard)
            }
            None => {
                self.set_state(GatewayState::Identified);
                self.info(&format!(
                    "发送 IDENTIFY intents={} shard=[{}, {}]",
                    self.config.intents, shard[0], shard[1]
                ));
                identify_frame(&token, self.config.intents, shard)
            }
        };
        if let Err(err) = io.send(Frame::Text(frame)).await {
            self.close_quiet(&io).await;
            return Err(err);
        }
        self.serve(io, interval_ms).await
    }

    async fn close_quiet(&self, io: &Arc<dyn FrameIo>) {
        let _ = tokio::time::timeout(Duration::from_millis(200), io.close(1000)).await;
    }

    async fn await_hello(&self, io: Arc<dyn FrameIo>) -> Result<u64, GatewayError> {
        loop {
            let frame = match tokio::time::timeout(self.config.handshake_timeout, io.recv()).await {
                Ok(Ok(frame)) => frame,
                Ok(Err(err)) => return Err(err),
                Err(_) => return Err(GatewayError::MissingHello),
            };
            match frame {
                Frame::Close(code) => return Err(self.close_error(code).await),
                other => {
                    if let Some(text) = other.text_of() {
                        if let Some(interval) = parse_hello(&text) {
                            self.info(&format!("收到 HELLO 心跳间隔={interval}ms"));
                            return Ok(interval);
                        }
                    }
                }
            }
        }
    }

    async fn close_error(&self, code: u16) -> GatewayError {
        self.note_close(Some(code));
        match classify(code) {
            CloseAction::Stop => GatewayError::Stopped(code),
            CloseAction::AuthFail => {
                self.tokens.invalidate().await;
                self.drop_session();
                GatewayError::InvalidSession
            }
            CloseAction::Reidentify => {
                self.drop_session();
                GatewayError::InvalidSession
            }
            CloseAction::Reconnect => GatewayError::Closed(code),
        }
    }

    async fn serve(
        self: &Arc<Self>,
        io: Arc<dyn FrameIo>,
        interval_ms: u64,
    ) -> Result<(), GatewayError> {
        let acked = Arc::new(AtomicBool::new(false));
        let (abort_tx, mut abort_rx) = watch::channel(false);
        let mut shutdown = self.shutdown_rx.clone();
        let mut heartbeat: Option<HeartbeatHandle> = None;
        let window = self.config.read_window;
        let outcome = loop {
            let recv = io.recv();
            let step = tokio::select! {
                _ = shutdown.changed() => Step::Shutdown,
                _ = abort_rx.changed() => Step::Aborted,
                frame = tokio::time::timeout(window, recv) => match frame {
                    Ok(Ok(frame)) => Step::Frame(frame),
                    Ok(Err(err)) => Step::Failed(err),
                    Err(_) => Step::Timeout,
                },
            };
            match step {
                Step::Shutdown => break Err(GatewayError::Shutdown),
                Step::Aborted => break Err(GatewayError::HeartbeatLost),
                Step::Timeout => break Err(GatewayError::ReadTimeout),
                Step::Failed(err) => break Err(err),
                Step::Frame(Frame::Close(code)) => break Err(self.close_error(code).await),
                Step::Frame(other) => {
                    let text = match other.text_of() {
                        Some(text) => text,
                        None => continue,
                    };
                    let event = match ProtocolEvent::from_text(&text) {
                        Some(event) => event,
                        None => continue,
                    };
                    let mut stop = None;
                    match event.op {
                        OP_HEARTBEAT_ACK => {
                            acked.store(true, Ordering::SeqCst);
                        }
                        OP_HEARTBEAT => {
                            let reply = Frame::Text(heartbeat_frame(self.seq()));
                            if io.send(reply).await.is_err() {
                                stop = Some(GatewayError::HeartbeatLost);
                            }
                        }
                        OP_RECONNECT => stop = Some(GatewayError::Reconnect),
                        OP_INVALID_SESSION => stop = Some(GatewayError::InvalidSession),
                        OP_HELLO => {}
                        OP_DISPATCH => {
                            self.bump_seq(event.seq);
                            if event.is_lifecycle() {
                                self.apply_lifecycle(&event);
                                if heartbeat.is_none() && interval_ms > 0 {
                                    heartbeat = Some(self.spawn_heartbeat(
                                        Arc::clone(&io),
                                        interval_ms,
                                        Arc::clone(&acked),
                                        abort_tx.clone(),
                                    ));
                                }
                            } else {
                                self.note_event();
                                self.sink.dispatch(event);
                            }
                        }
                        _ => {}
                    }
                    if let Some(err) = stop {
                        break Err(err);
                    }
                }
            }
        };
        if let Some(handle) = heartbeat {
            handle.shutdown().await;
        }
        self.close_quiet(&io).await;
        outcome
    }

    fn apply_lifecycle(&self, event: &ProtocolEvent) {
        if event.event_type.as_deref() == Some("READY") {
            self.apply_ready(&event.raw);
        } else {
            self.apply_resumed();
        }
    }

    fn spawn_heartbeat(
        self: &Arc<Self>,
        io: Arc<dyn FrameIo>,
        interval_ms: u64,
        acked: Arc<AtomicBool>,
        abort: watch::Sender<bool>,
    ) -> HeartbeatHandle {
        let (stop_tx, mut stop_rx) = watch::channel(false);
        acked.store(true, Ordering::SeqCst);
        let client = Arc::clone(self);
        let interval = Duration::from_millis(interval_ms);
        client.add_live_task();
        let join = tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {}
                    _ = stop_rx.changed() => break,
                }
                if !acked.swap(false, Ordering::SeqCst) {
                    let _ = abort.send(true);
                    break;
                }
                let frame = Frame::Text(heartbeat_frame(client.seq()));
                if io.send(frame).await.is_err() {
                    let _ = abort.send(true);
                    break;
                }
            }
            client.remove_live_task();
        });
        HeartbeatHandle {
            stop: stop_tx,
            join,
        }
    }
}

#[derive(Debug)]
enum Step {
    Shutdown,
    Aborted,
    Timeout,
    Failed(GatewayError),
    Frame(Frame),
}

struct HeartbeatHandle {
    stop: watch::Sender<bool>,
    join: JoinHandle<()>,
}

impl HeartbeatHandle {
    async fn shutdown(self) {
        let _ = self.stop.send(true);
        let abort = self.join.abort_handle();
        if tokio::time::timeout(Duration::from_secs(2), self.join)
            .await
            .is_err()
        {
            abort.abort();
        }
    }
}
