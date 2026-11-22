# aurorix-api API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/api.rs`

- `pub const DEFAULT_BASE_URL: &str = "https://api.bot.qq.com"`
- `pub struct BotConfig`
- `impl BotConfig`
  - `pub fn new(app_id: impl Into<String>, app_secret: impl Into<String>) -> Self`
  - `pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self`
  - `pub fn with_token_url(mut self, token_url: impl Into<String>) -> Self`
- `pub struct BotApi`
- `impl BotApi`
  - `pub fn new(config: BotConfig) -> Self`
  - `pub fn config(&self) -> &BotConfig`
  - `pub fn http(&self) -> &reqwest::Client`
  - `pub fn tokens(&self) -> &TokenManager`
  - `pub fn invalidate_token(&self)`
  - `pub fn endpoint(&self, path: &str) -> String`
  - `pub fn retry_when(&self) -> Arc<Vec<i32>>`
  - `pub fn set_retry_when(&self, codes: &[i32])`
  - `pub fn upload_threshold(&self) -> usize`
  - `pub fn set_upload_threshold(&self, threshold: usize)`
  - `pub fn audit(&self) -> &AuditRegistry`
  - `pub fn set_audit_timeout(&self, timeout: Duration)`
  - `pub fn resolve_audit(&self, audit_id: &str, message_id: &str, approved: bool) -> bool`
  - `pub fn limiter(&self) -> &RateLimiter`
  - `pub fn check_push_rate(&self) -> Result<(), ApiError>`
  - `pub fn recall_grace(&self) -> Duration`
  - `pub fn set_recall_grace(&self, grace: Duration)`
- `pub struct GroupInfo`
- `impl GroupInfo`
  - `pub fn name(&self) -> &str`
- `pub fn delivery_steps(target: &Target, message: &Outbound) -> Vec<DeliveryStep>`
- `pub fn passive_gone(err: &ApiError) -> bool`

## `src/audit.rs`

- `pub const AUDIT_TIMEOUT: Duration = Duration::from_secs(30)`
- `pub const AUDIT_PENDING_CODE: i32 = 304023`
- `pub struct AuditOutcome`
- `pub struct AuditRegistry`
- `impl AuditRegistry`
  - `pub fn new() -> Self`
  - `pub fn set_timeout(&self, timeout: Duration)`
  - `pub fn timeout(&self) -> Duration`
  - `pub fn resolve(&self, audit_id: &str, message_id: &str, approved: bool) -> bool`
  - `pub fn waiting(&self) -> usize`

## `src/dedup.rs`

- `pub const DEFAULT_DEDUP_TTL: Duration = Duration::from_secs(300)`
- `pub const DEFAULT_DEDUP_CAPACITY: usize = 8192`
- `pub struct DedupWindow`
- `impl DedupWindow`
  - `pub fn new(ttl: Duration) -> Self`
  - `pub fn with_capacity(ttl: Duration, capacity: usize) -> Self`
  - `pub fn capacity(&self) -> usize`
  - `pub fn with_default_ttl() -> Self`
  - `pub fn ttl(&self) -> Duration`
  - `pub fn check_and_mark(&self, event_id: &str) -> bool`
  - `pub fn check_and_mark_event(&self, event_id: &str, message_id: &str) -> bool`
  - `pub fn seen(&self, event_id: &str) -> bool`
  - `pub fn forget(&self, event_id: &str)`
  - `pub fn purge(&self) -> usize`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`

## `src/error.rs`

- `pub enum ErrorKind`
- `impl ErrorKind`
  - `pub const ALL: [ErrorKind; 9] = [`
  - `pub const fn label(self) -> &'static str`
  - `pub const fn classified(self) -> bool`
  - `pub const fn server_side(self) -> bool`
  - `pub const fn passive_expired(self) -> bool`
- `pub fn describe(code: i32) -> &'static str`
- `pub fn classify(code: i32) -> ErrorKind`
- `pub fn is_retryable(code: i32, extra: &[i32]) -> bool`
- `pub struct BusinessError`
- `impl BusinessError`
  - `pub fn new(code: i32, message: impl Into<String>) -> Self`
  - `pub fn kind(&self) -> ErrorKind`
  - `pub fn describe(&self) -> &'static str`
  - `pub fn is_passive_expired(&self) -> bool`
  - `pub fn is_retryable(&self, extra: &[i32]) -> bool`
- `pub enum ApiError`
- `impl ApiError`
  - `pub fn kind(&self) -> Option<ErrorKind>`
  - `pub fn code(&self) -> Option<i32>`
  - `pub fn is_passive_expired(&self) -> bool`
  - `pub fn is_retryable(&self, extra: &[i32]) -> bool`
  - `pub fn upload(message: impl Into<String>) -> Self`

## `src/interaction.rs`

- `pub const INTERACTION_ACK_CODE: i64 = 0`
- `pub const INTERACTION_ACK_PATH: &str = "/interactions/"`
- `pub fn interaction_path(interaction_id: &str) -> String`
- `impl BotApi`

## `src/lib.rs`

- `pub use api::{delivery_steps, passive_gone, BotApi, BotConfig, GroupInfo, DEFAULT_BASE_URL}`
- `pub use audit::{AuditOutcome, AuditRegistry, AUDIT_PENDING_CODE, AUDIT_TIMEOUT}`
- `pub use dedup::{DedupWindow, DEFAULT_DEDUP_TTL}`
- `pub use error::{classify, describe, is_retryable, ApiError, BusinessError, ErrorKind}`
- `pub use interaction::{interaction_path, INTERACTION_ACK_CODE, INTERACTION_ACK_PATH}`
- `pub use markdown::{ cmd_escape, encode_cmd_tags, fit_images, prepare, prepare_with, protect_at, BoxFuture, ImageStore, Prepared, PreparedImage, StoredImage, CHANNEL_TEXT_LIMIT, ZERO_WIDTH_SPACE, }`
- `pub use message::{ Anchor, DeliveryStep, Outbound, SendOutcome, SendResult, SeqBudget, MAX_PASSIVE_SEQ, }`
- `pub use panels::{ panel_display_width, valid_panel_scope, validate_panel, Panel, PanelItem, PanelPage, PanelRecord, PANEL_DESC_MAX_WIDTH, PANEL_ITEM_COMMAND, PANEL_ITEM_LINK, PANEL_MAX_ITEMS, PANEL_MAX_PAGES, PANEL_MAX_PER_BOT, PANEL_NAME_MAX_WIDTH, PANEL_PAGE_LIMIT, PANEL_REMARK_MAX_LEN, PANEL_TARGET_ALL, }`
- `pub use ratelimit::{RateLimiter, PUSH_KEY, PUSH_RATE_LIMIT, PUSH_RATE_WINDOW}`
- `pub use recall::{RecallFailure, RecallReport, SentBatch, RECALL_GRACE, RECALL_WINDOW}`
- `pub use text::{guild_text, split_images, strip_latex, truncate_runes, GUILD_TEXT_LIMIT}`
- `pub use token::{TokenManager, DEFAULT_TOKEN_URL, EXPIRY_MARGIN, FAILURE_COOLDOWN}`
- `pub use transport::{ EVENT_ID_RETRY_CODE, EVENT_ID_RETRY_DELAY, EVENT_ID_RETRY_LIMIT, RETRY_BASE_DELAY, RETRY_LIMIT, }`
- `pub use upload::{ md5_10m_hex, md5_hex, media_base, MediaResult, MediaUpload, DEFAULT_UPLOAD_THRESHOLD, MD5_HEAD_SIZE, MEDIA_FILE, MEDIA_IMAGE, MEDIA_VIDEO, MEDIA_VOICE, PART_ATTEMPTS, PART_RETRY_DELAY, }`
- `pub use aurorix_core::{Scene, Target}`
- `pub use aurorix_protocol::{MessageKind, SendRequest}`

## `src/markdown.rs`

- `pub use crate::text::strip_latex`
- `pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>`
- `pub struct StoredImage`
- `pub trait ImageStore: Send + Sync`
- `pub const ZERO_WIDTH_SPACE: &str = "\u{200b}"`
- `pub const CHANNEL_TEXT_LIMIT: usize = GUILD_TEXT_LIMIT`
- `pub struct PreparedImage`
- `impl PreparedImage`
  - `pub fn byte_direct(&self) -> bool`
- `pub struct Prepared`
- `pub fn protect_at(content: &str) -> String`
- `pub fn encode_cmd_tags(content: &str) -> String`
- `pub fn cmd_escape(value: &str) -> String`
- `pub fn fit_images(content: &str, width: u32, height: u32) -> String`

## `src/message.rs`

- `pub const MAX_PASSIVE_SEQ: u32 = 5`
- `pub struct Anchor`
- `impl Anchor`
  - `pub fn new(msg_id: impl Into<String>, event_id: impl Into<String>) -> Self`
  - `pub fn from_message(msg_id: impl Into<String>) -> Self`
  - `pub fn from_event(event_id: impl Into<String>) -> Self`
  - `pub fn empty(&self) -> bool`
- `pub struct SeqBudget`
- `impl SeqBudget`
  - `pub fn new() -> Self`
  - `pub fn shared() -> Arc<Self>`
  - `pub fn count(&self) -> Result<u32, ApiError>`
  - `pub fn spent(&self) -> bool`
  - `pub fn sent(&self) -> u32`
- `pub struct Outbound`
- `impl Outbound`
  - `pub fn text(content: impl Into<String>) -> Self`
  - `pub fn markdown(content: impl Into<String>) -> Self`
  - `pub fn media(content: impl Into<String>, media: Value) -> Self`
  - `pub fn with_keyboard(mut self, keyboard: &Keyboard) -> Result<Self, ApiError>`
  - `pub fn with_keyboard_json(mut self, keyboard: Value) -> Self`
  - `pub fn quote_to(mut self, message_id: impl Into<String>) -> Self`
  - `pub fn with_anchor(mut self, anchor: Anchor) -> Self`
  - `pub fn with_msg_id(self, msg_id: impl Into<String>) -> Self`
  - `pub fn with_event_id(self, event_id: impl Into<String>) -> Self`
  - `pub fn with_budget(mut self, budget: Arc<SeqBudget>) -> Self`
  - `pub fn disable_active_fallback(mut self) -> Self`
  - `pub fn as_initiative(mut self) -> Self`
- `pub struct SendResult`
- `impl SendResult`
  - `pub fn message_id(&self) -> Option<String>`
  - `pub fn audit(&self) -> Option<String>`
- `pub struct SendOutcome`
- `impl SendOutcome`
  - `pub fn recallable(&self) -> bool`
- `pub struct DeliveryStep`

## `src/panels.rs`

- `pub const PANEL_NAME_MAX_WIDTH: usize = 14`
- `pub const PANEL_DESC_MAX_WIDTH: usize = 30`
- `pub const PANEL_REMARK_MAX_LEN: usize = 255`
- `pub const PANEL_MAX_ITEMS: usize = 20`
- `pub const PANEL_MAX_PER_BOT: usize = 20`
- `pub const PANEL_ITEM_COMMAND: &str = "command"`
- `pub const PANEL_ITEM_LINK: &str = "link"`
- `pub const PANEL_TARGET_ALL: &str = "all"`
- `pub const PANEL_PAGE_LIMIT: usize = 50`
- `pub const PANEL_MAX_PAGES: usize = 40`
- `pub struct PanelItem`
- `pub struct Panel`
- `pub struct PanelRecord`
- `pub struct PanelPage`
- `pub fn panel_display_width(s: &str) -> usize`
- `pub fn validate_panel(panel: &Panel) -> Result<(), String>`
- `pub fn valid_panel_scope(scope: &str) -> bool`
- `impl crate::BotApi`

## `src/ratelimit.rs`

- `pub const PUSH_KEY: &str = "push"`
- `pub const PUSH_RATE_LIMIT: u32 = 20`
- `pub const PUSH_RATE_WINDOW: Duration = Duration::from_secs(60)`
- `pub struct RateLimiter`
- `impl RateLimiter`
  - `pub fn new() -> Self`
  - `pub fn without_rules() -> Self`
  - `pub fn configure(&self, key: impl Into<String>, limit: u32, window: Duration)`
  - `pub fn rule(&self, key: &str) -> Option<(u32, Duration)>`
  - `pub fn check(&self, key: &str) -> Result<(), ApiError>`
  - `pub fn remaining(&self, key: &str) -> u32`
  - `pub fn reset(&self, key: &str)`
  - `pub fn hits(&self, key: &str) -> usize`

## `src/recall.rs`

- `pub const RECALL_WINDOW: Duration = Duration::from_secs(120)`
- `pub const RECALL_GRACE: Duration = Duration::from_millis(400)`
- `pub struct SentBatch`
- `impl SentBatch`
  - `pub fn new(ids: Vec<String>, at: Instant) -> Self`
  - `pub fn now(ids: Vec<String>) -> Self`
  - `pub fn delivered_active(mut self) -> Self`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn within_window(&self, now: Instant) -> bool`
- `pub struct RecallFailure`
- `pub struct RecallReport`
- `impl RecallReport`
  - `pub fn attempted(&self) -> usize`
- `impl BotApi`
  - `pub fn recallable(&self, batch: &SentBatch) -> bool`

## `src/text.rs`

- `pub const GUILD_TEXT_LIMIT: usize = 2000`
- `pub fn split_images(content: &str) -> (Vec<String>, String)`
- `pub fn guild_text(content: &str) -> String`
- `pub fn tidy_guild_lines(text: &str) -> String`
- `pub fn decorative(line: &str) -> bool`
- `pub fn truncate_runes(text: &str, limit: usize) -> String`
- `pub fn render_cmd_tags(text: &str) -> String`
- `pub fn strip_latex(text: &str) -> String`

## `src/token.rs`

- `pub const DEFAULT_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken"`
- `pub const EXPIRY_MARGIN: Duration = Duration::from_secs(50)`
- `pub const FAILURE_COOLDOWN: Duration = Duration::from_secs(3)`
- `impl Cached`
- `pub struct TokenManager`
- `impl TokenManager`
  - `pub fn new( …`
  - `pub fn app_id(&self) -> &str`
  - `pub fn endpoint(&self) -> &str`
  - `pub fn invalidate(&self)`
- `pub fn parse_expires_in(raw: Option<&Value>) -> Result<u64, ApiError>`

## `src/transport.rs`

- `pub const EVENT_ID_RETRY_CODE: i32 = 40034025`
- `pub const EVENT_ID_RETRY_DELAY: Duration = Duration::from_millis(200)`
- `pub const EVENT_ID_RETRY_LIMIT: u32 = 3`
- `pub const RETRY_LIMIT: u32 = 2`
- `pub const RETRY_BASE_DELAY: Duration = Duration::from_millis(300)`
- `pub fn parse_business_error(raw: &[u8]) -> Option<BusinessError>`
- `impl BotApi`

## `src/upload.rs`

- `pub const DEFAULT_UPLOAD_THRESHOLD: usize = 3 << 20`
- `pub const MD5_HEAD_SIZE: usize = 10_002_432`
- `pub const PART_ATTEMPTS: u32 = 3`
- `pub const PART_RETRY_DELAY: Duration = Duration::from_millis(800)`
- `pub const MIN_BUDGET: Duration = Duration::from_secs(60)`
- `pub const MAX_BUDGET: Duration = Duration::from_secs(600)`
- `pub const UPLOAD_RATE_BYTES: usize = 256 << 10`
- `pub const PART_RATE_BYTES: usize = 128 << 10`
- `pub const FALLBACK_BLOCK_SIZE: usize = 1024 * 1024`
- `pub const MEDIA_IMAGE: i32 = 1`
- `pub const MEDIA_VIDEO: i32 = 2`
- `pub const MEDIA_VOICE: i32 = 3`
- `pub const MEDIA_FILE: i32 = 4`
- `pub struct MediaUpload`
- `impl MediaUpload`
  - `pub fn image(data: Vec<u8>, filename: impl Into<String>) -> Self`
  - `pub fn from_url(url: impl Into<String>, file_type: i32) -> Self`
  - `pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self`
  - `pub fn force_chunked(mut self) -> Self`
- `pub struct MediaResult`
- `pub fn md5_hex(data: &[u8]) -> String`
- `pub fn sha1_hex(data: &[u8]) -> String`
- `pub fn head_bytes(data: &[u8], limit: usize) -> &[u8]`
- `pub fn md5_10m_hex(data: &[u8]) -> String`
- `pub fn size_budget(size: usize, rate: usize) -> Duration`
- `pub fn upload_budget(size: usize) -> Duration`
- `pub fn part_timeout(size: usize) -> Duration`
- `pub fn media_file_name(file_type: i32, name: &str) -> String`
- `pub fn media_base(target: &Target) -> Result<String, ApiError>`
- `impl BotApi`

