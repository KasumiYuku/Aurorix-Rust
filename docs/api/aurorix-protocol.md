# aurorix-protocol API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/error.rs`

- `pub enum ProtocolError`
- `pub type ProtocolResult<T> = Result<T, ProtocolError>`

## `src/event.rs`

- `pub enum EventKind`
- `impl EventKind`
  - `pub const ALL: [EventKind; 13] = [`
  - `pub const fn as_str(self) -> &'static str`
  - `pub fn parse(name: &str) -> Option<Self>`
- `pub struct GatewayEvent`
- `impl GatewayEvent`
  - `pub fn op_kind(&self) -> Option<WebSocketOp>`
  - `pub fn is_dispatch(&self) -> bool`
  - `pub fn kind(&self) -> Option<EventKind>`
  - `pub fn event_id(&self) -> Option<&str>`
  - `pub fn sequence(&self) -> Option<i64>`

## `src/intents.rs`

- `pub const DEFAULT_INTENTS: [&str; 13] = [`
- `pub struct IntentSet`
- `impl IntentSet`
  - `pub fn from_names(names: &[String]) -> Self`
  - `pub fn all() -> Self`
  - `pub fn empty() -> Self`
  - `pub fn names(&self) -> Vec<String>`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn contains(&self, name: &str) -> bool`

## `src/lib.rs`

- `pub use error::{ProtocolError, ProtocolResult}`
- `pub use event::{EventKind, GatewayEvent}`
- `pub use intents::{IntentSet, DEFAULT_INTENTS}`
- `pub use message::{ MessageKind, SendRequest, APP_ACCESS_TOKEN_PATH, CHANNEL_MESSAGE_PATH, DM_MESSAGE_PATH, GROUP_MESSAGE_PATH, USER_MESSAGE_PATH, }`
- `pub use op::WebSocketOp`
- `pub use retry::{should_retry, RetryPolicy, DEFAULT_RETRY_WHEN}`
- `pub use aurorix_core::{Scene, Target}`

## `src/message.rs`

- `pub use aurorix_core::{Scene, Target}`
- `pub const GROUP_MESSAGE_PATH: &str = "/v2/groups/{group_openid}/messages"`
- `pub const USER_MESSAGE_PATH: &str = "/v2/users/{openid}/messages"`
- `pub const CHANNEL_MESSAGE_PATH: &str = "/channels/{channel_id}/messages"`
- `pub const DM_MESSAGE_PATH: &str = "/dms/{guild_id}/messages"`
- `pub const APP_ACCESS_TOKEN_PATH: &str = "/app/getAppAccessToken"`
- `pub enum MessageKind`
- `impl MessageKind`
  - `pub const ALL: [MessageKind; 3] = [`
  - `pub const fn code(self) -> u8`
  - `pub const fn from_code(code: u8) -> Option<Self>`
  - `pub const fn label(self) -> &'static str`
- `pub struct SendRequest`
- `pub struct MessageReference`
- `impl MessageReference`
  - `pub fn new(message_id: impl Into<String>) -> Self`
  - `pub fn ignoring_missing(mut self, ignore: bool) -> Self`
- `impl SendRequest`
  - `pub fn new(content: impl Into<String>, kind: MessageKind) -> Self`
  - `pub fn plain(content: impl Into<String>) -> Self`
  - `pub fn markdown(content: impl Into<String>, markdown: Value) -> Self`
  - `pub fn media(content: impl Into<String>, media: Value) -> Self`
  - `pub fn with_msg_id(mut self, msg_id: impl Into<String>) -> Self`
  - `pub fn with_msg_seq(mut self, msg_seq: u32) -> Self`
  - `pub fn with_event_id(mut self, event_id: impl Into<String>) -> Self`
  - `pub fn with_keyboard(mut self, keyboard: Value) -> Self`
  - `pub fn with_reference(mut self, message_id: impl Into<String>) -> Self`
  - `pub fn with_timestamp(mut self, timestamp: i64) -> Self`
  - `pub fn kind(&self) -> Option<MessageKind>`
  - `pub fn path_for_scene(scene: Scene, id: &str) -> String`

## `src/op.rs`

- `pub enum WebSocketOp`
- `impl WebSocketOp`
  - `pub const ALL: [WebSocketOp; 8] = [`
  - `pub const fn from_code(code: i32) -> Option<Self>`
  - `pub const fn code(self) -> i32`
  - `pub const fn label(self) -> &'static str`

## `src/retry.rs`

- `pub const DEFAULT_RETRY_WHEN: [i32; 2] = [11253, 630006]`
- `pub fn should_retry(code: i32, configured: &[i32]) -> bool`
- `pub struct RetryPolicy`
- `impl RetryPolicy`
  - `pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self`
  - `pub fn delay_for(&self, attempt: u32) -> Duration`
  - `pub fn delay_for_attempt(&self, attempt: u32) -> Result<Duration, ProtocolError>`
  - `pub fn attempts(&self) -> u32`

