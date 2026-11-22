# aurorix-admin API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/adapters.rs`

- `pub struct PgBans`
- `impl PgBans`
  - `pub fn new(store: BanStore) -> Self`
  - `pub fn store(&self) -> &BanStore`
- `pub struct PgMenu`
- `impl PgMenu`
  - `pub fn new(store: PanelStore) -> Self`
  - `pub fn store(&self) -> &PanelStore`
- `pub struct PgAssets`
- `impl PgAssets`
  - `pub fn new(store: AssetStore) -> Self`
  - `pub fn store(&self) -> &AssetStore`
- `pub struct PgInbox`
- `impl PgInbox`
  - `pub fn new(store: InboxStore) -> Self`
  - `pub fn with_sender(store: InboxStore, sender: Arc<dyn InboxSender>) -> Self`
  - `pub fn store(&self) -> &InboxStore`
  - `pub fn has_sender(&self) -> bool`
- `pub fn postgres_sources(db: &Arc<Db>) -> (PgBans, PgMenu, PgAssets, PgInbox)`

## `src/assets.rs`

- `pub struct AssetField`
- `pub struct AssetProvider`
- `pub struct AssetsView`
- `pub struct AssetProviderInput`
- `pub struct AssetsInput`
- `pub trait AssetSource: Send + Sync`
- `pub struct EmptyAssets`

## `src/auth.rs`

- `pub const DEFAULT_TOKEN_TTL_SECONDS: u64 = 24 * 60 * 60`
- `pub const TOKEN_BYTES: usize = 32`
- `pub const SESSION_COOKIE: &str = "px_admin"`
- `pub struct SessionStore`
- `impl SessionStore`
  - `pub fn new(ttl: Duration) -> Self`
  - `pub fn with_ttl_seconds(seconds: u64) -> Self`
  - `pub fn capacity(&self) -> usize`
  - `pub fn ttl(&self) -> Duration`
  - `pub fn ttl_seconds(&self) -> u64`
  - `pub fn issue(&self) -> (String, u64)`
  - `pub fn issue_for(&self, ttl: Duration) -> (String, u64)`
  - `pub fn validate(&self, token: &str) -> bool`
  - `pub fn revoke(&self, token: &str)`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
- `pub fn password_matches(expected: &str, provided: &str) -> bool`
- `pub fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String>`
- `pub fn cookie_token(headers: &axum::http::HeaderMap) -> Option<String>`
- `pub fn session_token(headers: &axum::http::HeaderMap) -> Option<String>`

## `src/bans.rs`

- `pub const DEFAULT_BAN_MESSAGE: &str = "您的账号因违反Bot使用协议已被封禁"`
- `pub const BAN_MESSAGE_LIMIT: usize = 2000`
- `pub struct BanEntry`
- `pub trait BanSource: Send + Sync`
- `pub struct EmptyBans`
- `pub struct BanRequest`

## `src/config.rs`

- `pub const WRITABLE_KEYS: &[&str] = &[`
- `pub const RESTART_KEYS: &[&str] = &[`
- `pub const SECRET_MASK: &str = "********"`
- `pub const SECRET_KEYS: &[&str] = &["admin_password", "secret"]`
- `pub struct ConfigGroup`
- `pub struct CoreField`
- `pub fn config_groups() -> &'static [ConfigGroup]`
- `pub fn core_specs() -> Vec<CoreField>`
- `pub fn core_spec(key: &str) -> Option<CoreField>`
- `pub fn field_value(config: &AppConfig, key: &str) -> (Value, bool)`
- `pub fn field_views(config: &AppConfig) -> Vec<Value>`
- `pub fn group_views() -> Vec<Value>`
- `pub fn is_secret_placeholder(value: &Value) -> bool`
- `pub fn masked_values(config: &AppConfig) -> Map<String, Value>`
- `pub fn config_view(config: &AppConfig) -> Value`
- `pub fn is_writable(key: &str) -> bool`
- `pub fn is_secret(key: &str) -> bool`
- `pub fn needs_restart(key: &str) -> bool`
- `pub fn is_hot(key: &str) -> bool`
- `pub fn display_prefixes(prefixes: &[String]) -> Vec<String>`

## `src/error.rs`

- `pub enum ApiError`
- `impl ApiError`
  - `pub fn status(&self) -> StatusCode`
  - `pub fn message(&self) -> &str`
- `pub fn not_found() -> ApiError`
- `pub fn invalid_body() -> ApiError`

## `src/inbox.rs`

- `pub const DEFAULT_PAGE_SIZE: usize = 50`
- `pub const MAX_PAGE_SIZE: usize = 200`
- `pub struct Conversation`
- `pub struct InboxMessage`
- `pub enum InboxSendError`
- `pub trait InboxSender: Send + Sync`
- `pub trait InboxSource: Send + Sync`
- `pub struct EmptyInbox`
- `pub struct ConversationsQuery`
- `pub struct MessageQuery`
- `pub struct SendRequest`

## `src/jobs.rs`

- `pub struct JobInfo`
- `pub trait JobSource: Send + Sync`
- `pub struct EmptyJobs`
- `pub struct PauseRequest`

## `src/lib.rs`

- `pub use assets::{AssetField, AssetProvider, AssetSource, AssetsInput, AssetsView, EmptyAssets}`
- `pub use adapters::{postgres_sources, PgAssets, PgBans, PgInbox, PgMenu}`
- `pub use auth::{ bearer_token, cookie_token, password_matches, session_token, SessionStore, DEFAULT_TOKEN_TTL_SECONDS, SESSION_COOKIE, }`
- `pub use bans::{BanEntry, BanSource, EmptyBans, BAN_MESSAGE_LIMIT, DEFAULT_BAN_MESSAGE}`
- `pub use config::{config_view, is_secret, is_writable, needs_restart, WRITABLE_KEYS}`
- `pub use error::ApiError`
- `pub use inbox::{ Conversation, EmptyInbox, InboxMessage, InboxSendError, InboxSender, InboxSource, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, }`
- `pub use jobs::{EmptyJobs, JobInfo, JobSource}`
- `pub use live::{ spawn_live_tasks, Envelope, LiveBus, LiveStream, TOPIC_INBOX, TOPIC_JOBS, TOPIC_LOG, TOPIC_OVERVIEW, }`
- `pub use menu::{ EmptyMenu, MenuSnapshot, MenuSource, Panel, PanelItem, PanelRecord, RestoredPanel, DEFAULT_SNAPSHOT_DIR, PANEL_MAX_ITEMS, PANEL_MAX_PER_BOT, PANEL_SCOPES, }`
- `pub use profile::{EmptyProfile, ProfileSource}`
- `pub use remote::{PanelRemote, PanelSyncReport, RemoteMenuSource}`
- `pub use routes::{ build_log_filter, clamp_limit, router, LogQuery, DEFAULT_LOG_LIMIT, MAX_LOG_LIMIT, REMEMBER_TTL_SECONDS, }`
- `pub use sources::{ default_access, empty_object, Counts, EmptyLogs, EmptyPlugins, EmptyStats, EmptyStatus, GatewayStatus, LogOverview, LogSource, PluginError, PluginInfo, PluginSource, SourceError, SourceFuture, StatsSource, StatusSource, StatusView, }`
- `pub use state::AdminState`
- `pub use stream::LogStream`
- `pub use system::{Control, ControlAction, UNSUPERVISED_RESTART_ERROR}`
- `pub use webui::{EmptyWebUis, PluginWebUi, WebUiSource, IMMUTABLE_CACHE, NO_CACHE}`

## `src/live.rs`

- `pub const TOPIC_OVERVIEW: &str = "overview.snapshot"`
- `pub const TOPIC_JOBS: &str = "jobs.list"`
- `pub const TOPIC_LOG: &str = "log.entry"`
- `pub const TOPIC_INBOX: &str = "inbox.message"`
- `pub const LIVE_BUFFER: usize = 256`
- `pub const OVERVIEW_INTERVAL: Duration = Duration::from_secs(2)`
- `pub struct Envelope`
- `pub struct LiveBus`
- `impl LiveBus`
  - `pub fn new() -> Self`
  - `pub fn shared() -> Arc<Self>`
  - `pub fn subscribers(&self) -> usize`
  - `pub fn publish(&self, topic: &str, data: Value)`
  - `pub fn subscribe(&self) -> broadcast::Receiver<Envelope>`
- `pub fn now_ms() -> i64`
- `pub struct LiveStream`
- `impl LiveStream`
  - `pub fn new(receiver: broadcast::Receiver<Envelope>) -> Self`
- `pub fn spawn_live_tasks(state: AdminState, bus: Arc<LiveBus>) -> tokio::task::JoinHandle<()>`

## `src/menu.rs`

- `pub const PANEL_NAME_MAX_WIDTH: usize = 14`
- `pub const PANEL_DESC_MAX_WIDTH: usize = 30`
- `pub const PANEL_REMARK_MAX_LEN: usize = 255`
- `pub const PANEL_MAX_ITEMS: usize = 20`
- `pub const PANEL_MAX_PER_BOT: usize = 20`
- `pub const PANEL_ITEM_COMMAND: &str = "command"`
- `pub const PANEL_ITEM_LINK: &str = "link"`
- `pub const PANEL_TARGET_ALL: &str = "all"`
- `pub const PANEL_DISPLAY_NOTE: &str = "长度按显示宽度算: 中文与全角标点各计 2"`
- `pub const PANEL_SCOPES: [(&str, &str); 4] = [`
- `pub const DEFAULT_SNAPSHOT_DIR: &str = "data/menu-snapshots"`
- `pub struct PanelItem`
- `pub struct Panel`
- `pub struct PanelRecord`
- `pub struct MenuSnapshot`
- `pub struct RestoredPanel`
- `pub trait MenuSource: Send + Sync`
- `pub struct EmptyMenu`
- `pub fn scope_label(scope: &str) -> &'static str`
- `pub fn display_width(text: &str) -> usize`
- `pub fn validate_panel(panel: &Panel) -> Result<(), String>`
- `pub fn editable(record: &PanelRecord) -> bool`
- `pub struct CreatePanelRequest`
- `pub struct UpdatePanelRequest`
- `pub struct SyncTarget`
- `pub struct SyncRequest`
- `pub struct SyncResult`

## `src/profile.rs`

- `pub trait ProfileSource: Send + Sync`
- `pub struct EmptyProfile`

## `src/remote.rs`

- `pub trait PanelRemote: Send + Sync`
- `pub struct PanelSyncReport`
- `pub struct RemoteMenuSource`
- `impl RemoteMenuSource`
  - `pub fn new(local: Arc<dyn MenuSource>) -> Self`
  - `pub fn with_remote(mut self, remote: Arc<dyn PanelRemote>) -> Self`
  - `pub fn with_logger(mut self, logger: Arc<Logger>) -> Self`
  - `pub fn remote_enabled(&self) -> bool`
  - `pub fn sync_all_async(&self) -> SourceFuture<'_, Result<PanelSyncReport, SourceError>>`

## `src/routes.rs`

- `pub const MAX_LOG_LIMIT: usize = 2048`
- `pub const DEFAULT_LOG_LIMIT: usize = 500`
- `pub const MODULE: &str = "admin"`
- `pub const REMEMBER_TTL_SECONDS: u64 = 30 * 24 * 60 * 60`
- `pub fn router(state: AdminState) -> Router`
- `impl LoginRequest`
- `pub struct LogQuery`
- `pub fn clamp_limit(limit: Option<i64>) -> usize`
- `pub fn build_log_filter(query: &LogQuery) -> Result<RingFilter, ApiError>`

## `src/sources.rs`

- `pub type SourceFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>`
- `pub enum SourceError`
- `impl SourceError`
  - `pub fn status(&self) -> StatusCode`
  - `pub fn message(&self) -> &str`
  - `pub fn from_db(err: DbError) -> Self`
  - `pub fn from_message(message: String) -> Self`
- `pub struct GatewayStatus`
- `pub struct Counts`
- `pub struct StatusView`
- `pub struct LogOverview`
- `pub struct PluginInfo`
- `pub fn empty_object() -> serde_json::Value`
- `pub fn default_access() -> serde_json::Value`
- `pub enum PluginError`
- `pub trait StatusSource: Send + Sync`
- `pub trait StatsSource: Send + Sync`
- `pub trait LogSource: Send + Sync`
- `pub trait PluginSource: Send + Sync`
- `pub struct EmptyStatus`
- `pub struct EmptyStats`
- `pub struct EmptyPlugins`
- `pub struct EmptyLogs`

## `src/state.rs`

- `pub struct AdminState`
- `impl AdminState`
  - `pub fn new(config_path: impl AsRef<Path>, config: AppConfig) -> Self`
  - `pub fn with_status(mut self, status: Arc<dyn StatusSource>) -> Self`
  - `pub fn with_stats(mut self, stats: Arc<dyn StatsSource>) -> Self`
  - `pub fn with_logs(mut self, logs: Arc<dyn LogSource>) -> Self`
  - `pub fn with_plugins(mut self, plugins: Arc<dyn PluginSource>) -> Self`
  - `pub fn with_menu(mut self, menu: Arc<dyn MenuSource>) -> Self`
  - `pub fn with_bans(mut self, bans: Arc<dyn BanSource>) -> Self`
  - `pub fn with_inbox(mut self, inbox: Arc<dyn InboxSource>) -> Self`
  - `pub fn with_jobs(mut self, jobs: Arc<dyn JobSource>) -> Self`
  - `pub fn with_assets(mut self, assets: Arc<dyn AssetSource>) -> Self`
  - `pub fn with_profile(mut self, profile: Arc<dyn ProfileSource>) -> Self`
  - `pub fn with_webui(mut self, webui: Arc<dyn WebUiSource>) -> Self`
  - `pub fn with_database(self, db: Arc<aurorix_db::Db>) -> Self`
  - `pub fn with_control(mut self, control: Control) -> Self`
  - `pub fn with_live(mut self, live: Arc<LiveBus>) -> Self`
  - `pub fn with_logger(mut self, logger: Arc<Logger>) -> Self`
  - `pub fn with_token_ttl(mut self, ttl: Duration) -> Self`
  - `pub fn with_token_ttl_seconds(self, seconds: u64) -> Self`
  - `pub fn lock_config(&self) -> MutexGuard<'_, AppConfig>`
  - `pub fn config_snapshot(&self) -> AppConfig`
  - `pub fn password(&self) -> String`
  - `pub fn config_file(&self) -> &Path`

## `src/stream.rs`

- `pub struct LogStream`
- `impl LogStream`
  - `pub fn new(receiver: broadcast::Receiver<LogRecord>) -> Self`

## `src/system.rs`

- `pub const UNSUPERVISED_RESTART_ERROR: &str = "未声明受外部守护(AURORIX_SUPERVISED=1), 面板重启会与守护进程各拉一个实例互杀; 请手动重启或在受管模式下使用"`
- `pub const CONTROL_DELAY_MS: u64 = 300`
- `pub type ControlAction = Arc<dyn Fn() + Send + Sync>`
- `pub struct Control`
- `impl Control`
  - `pub fn supervised(restart: Option<ControlAction>, stop: Option<ControlAction>) -> Self`

## `src/webui.rs`

- `pub const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable"`
- `pub const NO_CACHE: &str = "no-cache"`
- `pub const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8"`
- `pub const ASSET_EXTENSIONS: [&str; 20] = [`
- `pub struct PluginWebUi`
- `impl PluginWebUi`
  - `pub fn new(id: impl Into<String>) -> Self`
  - `pub fn with_api(mut self, api: Router) -> Self`
  - `pub fn with_spa_root(mut self, root: impl Into<PathBuf>) -> Self`
- `pub trait WebUiSource: Send + Sync`
- `pub struct EmptyWebUis`
- `pub fn extension(name: &str) -> String`
- `pub fn is_asset(name: &str) -> bool`
- `pub fn content_type(name: &str) -> &'static str`
- `pub fn mount_spa(app: Router, id: &str, root: PathBuf) -> Router`
- `pub fn mount_missing(app: Router, id: &str) -> Router`
- `pub const DIST_INDEX_NAME: &str = "index.html"`
- `pub const DIST_ADMIN_PREFIX: &str = "admin/"`
- `pub const DIST_API_PREFIX: &str = "api"`
- `pub struct EmbeddedAsset`
- `pub fn embedded_asset(name: &str) -> Option<&'static EmbeddedAsset>`
- `pub fn spa_path(raw: &str) -> Option<String>`
- `pub fn is_api_path(name: &str) -> bool`
- `pub fn index_page() -> Response`
- `pub fn serve_embedded(raw_path: &str) -> Response`

