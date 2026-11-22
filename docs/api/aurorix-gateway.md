# aurorix-gateway API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/backoff.rs`

- `pub const BASE_DELAY: Duration = Duration::from_millis(1_000)`
- `pub const MAX_DELAY: Duration = Duration::from_millis(30_000)`
- `pub struct Backoff`
- `impl Backoff`
  - `pub fn new(base: Duration, max: Duration) -> Self`
  - `pub fn default_policy() -> Self`
  - `pub fn base(&self) -> Duration`
  - `pub fn max(&self) -> Duration`
  - `pub fn current(&self) -> Duration`
  - `pub fn reset(&mut self)`
  - `pub fn advance(&mut self) -> Duration`
  - `pub fn next_delay(&mut self) -> Duration`
- `pub fn double(current: Duration, max: Duration) -> Duration`
- `pub fn jitter(delay: Duration) -> Duration`

## `src/client.rs`

- `pub const DEFAULT_GATEWAY_URL: &str = "wss://api.sgroup.qq.com/websocket"`
- `pub const GATEWAY_MODULE: &str = "gateway"`
- `pub struct GatewayConfig`
- `impl GatewayConfig`
  - `pub fn new(url: impl Into<String>, intents: u64) -> Self`
  - `pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self`
  - `pub fn with_backoff(mut self, base: Duration, max: Duration) -> Self`
  - `pub fn with_read_window(mut self, window: Duration) -> Self`
  - `pub fn with_reauth_delay(mut self, delay: Duration) -> Self`
  - `pub fn with_strikes(mut self, strikes: u32) -> Self`
  - `pub fn with_shard(mut self, shard: [u32; 2]) -> Self`
- `pub struct GatewayClient`
- `impl GatewayClient`
  - `pub fn new( …`
  - `pub fn with_transport( …`
  - `pub fn with_logger(mut self, logger: Arc<Logger>) -> Self`
  - `pub fn info(&self, message: &str)`
  - `pub fn warn(&self, message: &str)`
  - `pub fn config(&self) -> &GatewayConfig`
  - `pub fn state(&self) -> GatewayState`
  - `pub fn stats(&self) -> GatewayStats`
  - `pub fn live_tasks(&self) -> usize`
  - `pub fn is_stopped(&self) -> bool`
  - `pub fn is_shutdown(&self) -> bool`
  - `pub fn session_id(&self) -> Option<String>`
  - `pub fn seq(&self) -> i64`
  - `pub fn ready_count(&self) -> u64`
  - `pub fn start(self: &Arc<Self>)`
- `impl HeartbeatHandle`

## `src/codes.rs`

- `pub const AUTH_FAIL: u16 = 4004`
- `pub const SESSION_GONE: u16 = 4006`
- `pub const INVALID_SEQ: u16 = 4007`
- `pub const BOT_OFFLINE: u16 = 4914`
- `pub const BOT_BANNED: u16 = 4915`
- `pub enum CloseAction`
- `pub fn can_resume(code: u16) -> bool`
- `pub fn need_reidentify(code: u16) -> bool`
- `pub fn classify(code: u16) -> CloseAction`
- `pub fn is_permanent(code: u16) -> bool`

## `src/error.rs`

- `pub enum GatewayError`
- `impl GatewayError`
  - `pub fn is_stop(&self) -> bool`
  - `pub fn is_shutdown(&self) -> bool`
  - `pub fn close_code(&self) -> Option<u16>`

## `src/frame.rs`

- `pub const HEARTBEAT_CEILING_MS: u64 = 45_000`
- `pub const TOKEN_PREFIX: &str = "QQBot "`
- `pub const OP_DISPATCH: i32 = 0`
- `pub const OP_HEARTBEAT: i32 = 1`
- `pub const OP_IDENTIFY: i32 = 2`
- `pub const OP_RESUME: i32 = 6`
- `pub const OP_RECONNECT: i32 = 7`
- `pub const OP_INVALID_SESSION: i32 = 9`
- `pub const OP_HELLO: i32 = 10`
- `pub const OP_HEARTBEAT_ACK: i32 = 11`
- `pub const OP_CALLBACK_VALIDATE: i32 = 13`
- `pub struct Properties`
- `pub struct IdentifyPayload`
- `pub struct ResumePayload`
- `pub fn auth_token(token: &str) -> String`
- `pub fn identify_frame(token: &str, intents: u64, shard: [u32; 2]) -> String`
- `pub fn resume_frame(token: &str, session_id: &str, seq: i64, shard: [u32; 2]) -> String`
- `pub fn heartbeat_frame(seq: i64) -> String`
- `pub fn heartbeat_interval_ms(raw: u64) -> u64`
- `pub struct ProtocolEvent`
- `impl ProtocolEvent`
  - `pub fn kind(&self) -> Option<EventKind>`
  - `pub fn is_known(&self) -> bool`
  - `pub fn is_lifecycle(&self) -> bool`
  - `pub fn data_text(&self) -> String`
  - `pub fn from_text(text: &str) -> Option<ProtocolEvent>`
  - `pub fn from_bytes(bytes: &[u8]) -> Option<ProtocolEvent>`
- `pub fn parse_hello(text: &str) -> Option<u64>`
- `pub struct ReadyInfo`
- `pub fn parse_ready(text: &str) -> ReadyInfo`
- `pub fn parse_resumed_session(text: &str) -> Option<String>`

## `src/intents.rs`

- `pub const GUILDS: u64 = 1 << 0`
- `pub const GUILD_MEMBERS: u64 = 1 << 1`
- `pub const GUILD_MESSAGES: u64 = 1 << 9`
- `pub const DIRECT_MESSAGE: u64 = 1 << 12`
- `pub const GROUP_MEMBERS: u64 = 1 << 24`
- `pub const USER_MESSAGE: u64 = 1 << 25`
- `pub const INTERACTIONS: u64 = 1 << 26`
- `pub const MESSAGE_AUDIT: u64 = 1 << 27`
- `pub const PUBLIC_MESSAGES: u64 = 1 << 30`
- `pub const DEFAULT_INTENT_BITS: u64 = USER_MESSAGE | INTERACTIONS | GROUP_MEMBERS | MESSAGE_AUDIT`
- `pub const GUILD_INTENT_BITS: u64 =`
- `pub const EVENT_INTENTS: [(&str, u64); 28] = [`
- `pub const INTENT_EVENTS: [&str; 28] = [`
- `pub enum Protocol`
- `impl Protocol`
  - `pub fn parse(raw: &str) -> Option<Protocol>`
  - `pub fn as_str(self) -> &'static str`
  - `pub fn requires_websocket(self, names: &[String]) -> bool`
- `pub fn intent_bit(name: &str) -> u64`
- `pub fn bits_for_names(names: &[String]) -> u64`
- `pub fn bits_for_set(set: &IntentSet) -> u64`
- `pub fn requires_websocket(names: &[String]) -> bool`
- `pub fn ensure_protocol(protocol: Protocol, names: &[String]) -> Result<(), GatewayError>`
- `pub fn unknown_names(names: &[String]) -> Vec<String>`

## `src/lib.rs`

- `pub use backoff::{double, jitter, Backoff, BASE_DELAY, MAX_DELAY}`
- `pub use client::{GatewayClient, GatewayConfig, DEFAULT_GATEWAY_URL}`
- `pub use codes::{ can_resume, classify, is_permanent, need_reidentify, CloseAction, AUTH_FAIL, BOT_BANNED, BOT_OFFLINE, INVALID_SEQ, SESSION_GONE, }`
- `pub use error::GatewayError`
- `pub use frame::{ auth_token, heartbeat_frame, heartbeat_interval_ms, identify_frame, parse_hello, parse_ready, parse_resumed_session, resume_frame, ProtocolEvent, ReadyInfo, HEARTBEAT_CEILING_MS, OP_CALLBACK_VALIDATE, OP_DISPATCH, OP_HEARTBEAT, OP_HEARTBEAT_ACK, OP_HELLO, OP_IDENTIFY, OP_INVALID_SESSION, OP_RECONNECT, OP_RESUME, TOKEN_PREFIX, }`
- `pub use intents::{ bits_for_names, bits_for_set, ensure_protocol, intent_bit, requires_websocket, unknown_names, Protocol, DEFAULT_INTENT_BITS, EVENT_INTENTS, GUILD_INTENT_BITS, INTENT_EVENTS, }`
- `pub use sink::BroadcastSink`
- `pub use source::{fixed_headers, BoxFuture, EventSink, StaticTokenSource, TokenSource}`
- `pub use state::{GatewayState, GatewayStats}`
- `pub use transport::{ConnectRequest, Frame, FrameIo, Transport, WebSocketTransport}`
- `pub use webhook::{ callback_signature, ensure_webhook_compatible, raw_timestamp, signing_key, signing_seed, verify_signature, verifying_key, webhook_router, WebhookConfig, WebhookOutcome, WebhookProcessor, WebhookReply, SIGNATURE_HEADER, TIMESTAMP_HEADER, WEBHOOK_PATH, }`

## `src/sink.rs`

- `pub struct BroadcastSink`
- `impl BroadcastSink`
  - `pub fn new(capacity: usize) -> Self`
  - `pub fn subscribe(&self) -> broadcast::Receiver<Arc<ProtocolEvent>>`
  - `pub fn receivers(&self) -> usize`
  - `pub fn send(&self, event: ProtocolEvent) -> usize`

## `src/source.rs`

- `pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>`
- `pub trait TokenSource: Send + Sync`
- `pub trait EventSink: Send + Sync`
- `pub struct StaticTokenSource`
- `impl StaticTokenSource`
  - `pub fn new(token: impl Into<String>) -> Self`
  - `pub fn invalidations(&self) -> u64`
  - `pub fn set(&self, token: impl Into<String>)`
- `pub fn fixed_headers(app_id: &str) -> Vec<(String, String)>`

## `src/state.rs`

- `pub enum GatewayState`
- `impl GatewayState`
  - `pub fn as_str(self) -> &'static str`
  - `pub fn is_ready(self) -> bool`
  - `pub fn is_terminal(self) -> bool`
- `pub struct GatewayStats`

## `src/transport.rs`

- `pub enum Frame`
- `impl Frame`
  - `pub fn close_code(&self) -> Option<u16>`
  - `pub fn as_text(&self) -> Option<&str>`
  - `pub fn text_of(&self) -> Option<String>`
- `pub trait FrameIo: Send + Sync`
- `pub struct ConnectRequest`
- `impl ConnectRequest`
  - `pub fn new(url: impl Into<String>) -> Self`
  - `pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self`
- `pub trait Transport: Send + Sync`
- `pub struct WebSocketTransport`
- `impl WebSocketTransport`
  - `pub fn new() -> Self`
- `pub struct WsIo`
- `impl WsIo`

## `src/webhook.rs`

- `pub const WEBHOOK_PATH: &str = "/webhook"`
- `pub const SIGNATURE_HEADER: &str = "X-Signature-Ed25519"`
- `pub const TIMESTAMP_HEADER: &str = "X-Signature-Timestamp"`
- `pub const SEED_LEN: usize = 32`
- `pub const SIGNATURE_LEN: usize = 64`
- `pub const SIGNATURE_HIGH_BITS: u8 = 224`
- `pub fn signing_seed(secret: &str) -> [u8; SEED_LEN]`
- `pub fn signing_key(secret: &str) -> SigningKey`
- `pub fn verifying_key(secret: &str) -> VerifyingKey`
- `pub fn verify_signature( …`
- `pub fn raw_timestamp(value: &Value) -> String`
- `pub fn callback_signature(secret: &str, event_ts: &str, plain_token: &str) -> String`
- `pub struct WebhookReply`
- `impl WebhookReply`
  - `pub fn accepted() -> Self`
  - `pub fn payload(body: Value) -> Self`
  - `pub fn unauthorized() -> Self`
  - `pub fn bad_request() -> Self`
- `pub struct WebhookOutcome`
- `pub struct WebhookConfig`
- `impl WebhookConfig`
  - `pub fn new(app_secret: impl Into<String>, subscriptions: Vec<String>) -> Self`
  - `pub fn validate(&self) -> Result<(), GatewayError>`
- `pub fn ensure_webhook_compatible(names: &[String]) -> Result<(), GatewayError>`
- `pub struct WebhookProcessor`
- `impl WebhookProcessor`
  - `pub fn new(secret: impl Into<String>, sink: Arc<dyn EventSink>) -> Self`
  - `pub fn secret(&self) -> &str`
  - `pub fn sink(&self) -> Arc<dyn EventSink>`
  - `pub fn handle( …`
- `pub fn webhook_router(processor: Arc<WebhookProcessor>) -> Router`

