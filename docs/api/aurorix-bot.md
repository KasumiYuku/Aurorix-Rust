# aurorix-bot API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/bot.rs`

- `pub const MODULE: &str = "bot"`
- `pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5)`
- `pub const ADMIN_DRAIN: Duration = Duration::from_millis(500)`
- `pub const MEMORY_HEARTBEAT: Duration = Duration::from_secs(60)`
- `pub const DEFAULT_EVENT_BUFFER: usize = 1024`
- `pub const CONTROL_BUFFER: usize = 8`
- `pub const SUPERVISED_ENV: &str = "AURORIX_SUPERVISED"`
- `pub const UNSUPERVISED_RESTART_ERROR: &str = aurorix_admin::UNSUPERVISED_RESTART_ERROR`
- `pub enum FallbackPolicy`
- `pub enum ControlCommand`
- `impl ControlCommand`
  - `pub const fn label(self) -> &'static str`
- `pub struct ShutdownReport`
- `pub struct Runtime`
- `impl Runtime`
  - `pub fn config(&self) -> &Arc<BotConfig>`
  - `pub fn plugin_config(&self) -> &Arc<PluginConfigStore>`
  - `pub fn template_reload_count(&self) -> u64`
  - `pub fn template_reload_active(&self) -> bool`
  - `pub fn logger(&self) -> &Arc<Logger>`
  - `pub fn pipeline(&self) -> &Arc<Pipeline>`
  - `pub fn gateway(&self) -> &Arc<GatewayClient>`
  - `pub fn gateway_state(&self) -> GatewayState`
  - `pub fn counters(&self) -> PipelineCounters`
  - `pub fn live_tasks(&self) -> usize`
  - `pub fn uptime(&self) -> Duration`
  - `pub fn config_failures(&self) -> Vec<String>`
  - `pub fn outcome(&self) -> ControlCommand`
  - `pub fn is_finished(&self) -> bool`
  - `pub fn is_webhook(&self) -> bool`
  - `pub fn release_instance_lock(&self)`
- `pub struct RuntimeHandle`
- `impl RuntimeHandle`
  - `pub fn new(runtime: Arc<Runtime>) -> Self`
  - `pub fn runtime(&self) -> &Arc<Runtime>`
  - `pub fn pipeline(&self) -> &Arc<Pipeline>`
  - `pub fn logger(&self) -> &Arc<Logger>`
  - `pub fn live_tasks(&self) -> usize`
  - `pub fn gateway_state(&self) -> GatewayState`
  - `pub fn counters(&self) -> PipelineCounters`
  - `pub fn config_failures(&self) -> Vec<String>`
  - `pub fn is_finished(&self) -> bool`
  - `pub fn send(&self, command: ControlCommand) -> bool`
  - `pub fn request_stop(&self) -> bool`
  - `pub fn request_restart(&self) -> bool`
- `pub struct Bot`
- `impl Bot`
  - `pub fn builder(config: BotConfig) -> BotBuilder`
  - `pub fn config(&self) -> &Arc<BotConfig>`
  - `pub fn logger(&self) -> &Arc<Logger>`
  - `pub fn pipeline(&self) -> &Arc<Pipeline>`
  - `pub fn handle(&self) -> RuntimeHandle`
  - `pub fn is_webhook(&self) -> bool`
  - `pub fn live_tasks(&self) -> usize`
  - `pub fn gateway_state(&self) -> GatewayState`
  - `pub fn config_failures(&self) -> Vec<String>`
  - `pub fn counters(&self) -> PipelineCounters`
- `pub struct BotBuilder`
- `impl BotBuilder`
  - `pub fn new(config: BotConfig) -> Self`
  - `pub fn config(mut self, config: BotConfig) -> Self`
  - `pub fn allow_fallbacks(mut self) -> Self`
  - `pub fn fallback_policy(mut self, policy: FallbackPolicy) -> Self`
  - `pub fn with_transport(mut self, transport: Arc<dyn ReplyTransport>) -> Self`
  - `pub fn with_api(self, api: Arc<BotApi>) -> Self`
  - `pub fn with_tokens(mut self, tokens: Arc<dyn TokenSource>) -> Self`
  - `pub fn with_gateway_transport(mut self, transport: Arc<dyn Transport>) -> Self`
  - `pub fn with_kv(mut self, kv: Arc<dyn KvBackend>) -> Self`
  - `pub fn with_inbox(mut self, inbox: Arc<dyn InboxArchive>) -> Self`
  - `pub fn with_stats(mut self, stats: Arc<dyn StatsRecorder>) -> Self`
  - `pub fn with_plugins(mut self, registry: Arc<CommandRegistry>) -> Self`
  - `pub fn with_database(mut self, database: Arc<aurorix_db::Db>) -> Self`
  - `pub fn with_render(mut self, renderer: Arc<dyn Renderer>) -> Self`
  - `pub fn with_images(mut self, images: Arc<dyn aurorix_api::ImageStore>) -> Self`
  - `pub fn with_footers(mut self, footers: Arc<aurorix_buttons::FooterHub>) -> Self`
  - `pub fn with_jobs(mut self, jobs: Arc<aurorix_plugin::JobRegistry>) -> Self`
  - `pub fn with_logger(mut self, logger: Arc<Logger>) -> Self`
  - `pub fn with_templates(mut self, templates: Arc<TemplateRegistry>) -> Self`
  - `pub fn with_plugin_dbs(mut self, registry: Arc<aurorix_db::PluginDbRegistry>) -> Self`
  - `pub fn with_plugin_assets(mut self, book: Arc<aurorix_plugin::PluginAssetBook>) -> Self`
  - `pub fn with_template_override(mut self, dir: impl Into<String>) -> Self`
  - `pub fn with_roles(mut self, roles: Arc<dyn RoleResolver>) -> Self`
  - `pub fn with_plugin_config(mut self, sink: Arc<dyn PluginConfigurable>) -> Self`
  - `pub fn with_config_path(mut self, path: impl Into<std::path::PathBuf>) -> Self`
  - `pub fn with_panel_remote(mut self, remote: Arc<dyn aurorix_admin::PanelRemote>) -> Self`
  - `pub fn with_signal_handling(mut self, enabled: bool) -> Self`
  - `pub fn with_event_buffer(mut self, capacity: usize) -> Self`
  - `pub fn with_dedup_ttl(mut self, ttl: Duration) -> Self`
  - `pub fn with_instance_lock(mut self, enabled: bool) -> Self`
  - `pub fn build(self) -> Result<Bot, BotError>`
- `impl AdminStats`
- `pub struct AdminPlugins`
- `impl AdminPlugins`
  - `pub fn new(registry: Arc<CommandRegistry>, config: Arc<PluginConfigStore>) -> Self`
- `pub fn role_label(role: Option<RoleRequired>) -> &'static str`

## `src/config.rs`

- `pub const DEFAULT_ADMIN_PORT: u16 = 8080`
- `pub const DEFAULT_ADMIN_LISTEN: &str = "0.0.0.0"`
- `pub const DEFAULT_RENDER_POOL: usize = 4`
- `pub const DEFAULT_LOG_LEVEL: &str = "info"`
- `pub const DEFAULT_PROXY_API: &str = "https://api.bot.qq.com"`
- `pub const DEFAULT_PROTOCOL: &str = "websocket"`
- `pub const LOG_LEVELS: [&str; 4] = ["debug", "info", "warn", "error"]`
- `pub struct BotConfig`
- `impl BotConfig`
  - `pub fn new(appid: impl Into<String>, secret: impl Into<String>) -> Self`
  - `pub fn adaptive_render_pool() -> usize`
  - `pub fn from_app_config(app: &AppConfig) -> Self`
  - `pub fn with_gateway_url(mut self, url: impl Into<String>) -> Self`
  - `pub fn with_proxy_api(mut self, url: impl Into<String>) -> Self`
  - `pub fn with_admin(mut self, listen: impl Into<String>, port: u16) -> Self`
  - `pub fn with_render_pool(mut self, workers: usize) -> Self`
  - `pub fn with_intents(mut self, intents: Vec<String>) -> Self`
  - `pub fn with_maintenance(mut self, text: Option<String>) -> Self`
  - `pub fn footer_enabled(&self) -> bool`
  - `pub fn footer_global_lines(&self) -> Vec<String>`
  - `pub fn footer_scene_lines(&self, scene: aurorix_core::Scene) -> Vec<String>`
  - `pub fn welcome_on(&self) -> bool`
  - `pub fn farewell_on(&self) -> bool`
  - `pub fn welcome_lines(&self) -> Vec<String>`
  - `pub fn farewell_lines(&self) -> Vec<String>`
  - `pub fn welcome_extra_text(&self) -> &str`
  - `pub fn farewell_extra_text(&self) -> &str`
  - `pub fn welcome_button_lines(&self) -> Vec<String>`
  - `pub fn farewell_button_lines(&self) -> Vec<String>`
  - `pub fn with_footer_buttons(mut self, lines: Vec<String>) -> Self`
  - `pub fn with_lock_file(mut self, path: impl Into<String>) -> Self`
  - `pub fn lock_file_path(&self) -> String`
  - `pub fn admin_addr(&self) -> String`
  - `pub fn intent_bits(&self) -> u64`
  - `pub fn needs_websocket(&self) -> bool`
  - `pub fn level(&self) -> Result<Level, BotError>`
  - `pub fn validate(&self) -> Result<(), BotError>`
  - `pub fn gateway_config(&self) -> GatewayConfig`
  - `pub fn api_config(&self) -> aurorix_api::BotConfig`
  - `pub fn scheduler_config(&self) -> SchedulerConfig`

## `src/error.rs`

- `pub const FALLBACK_HINT: &str =`
- `pub enum BotError`
- `impl BotError`
  - `pub fn missing(name: &'static str, detail: impl Into<String>) -> Self`
  - `pub fn hint_text() -> &'static str`

## `src/event.rs`

- `pub const GROUP_AT_MESSAGE_CREATE: &str = "GROUP_AT_MESSAGE_CREATE"`
- `pub const GROUP_MESSAGE_CREATE: &str = "GROUP_MESSAGE_CREATE"`
- `pub const C2C_MESSAGE_CREATE: &str = "C2C_MESSAGE_CREATE"`
- `pub const AT_MESSAGE_CREATE: &str = "AT_MESSAGE_CREATE"`
- `pub const MESSAGE_CREATE: &str = "MESSAGE_CREATE"`
- `pub const DIRECT_MESSAGE_CREATE: &str = "DIRECT_MESSAGE_CREATE"`
- `pub const INTERACTION_CREATE: &str = "INTERACTION_CREATE"`
- `pub const MESSAGE_AUDIT_PASS: &str = "MESSAGE_AUDIT_PASS"`
- `pub const MESSAGE_AUDIT_REJECT: &str = "MESSAGE_AUDIT_REJECT"`
- `pub const GROUP_JOIN_REQUEST: &str = "GROUP_JOIN_REQUEST"`
- `pub const FRIEND_ADD: &str = "FRIEND_ADD"`
- `pub const FRIEND_DEL: &str = "FRIEND_DEL"`
- `pub const READY: &str = "READY"`
- `pub const RESUMED: &str = "RESUMED"`
- `pub const MESSAGE_EVENTS: [&str; 6] = [`
- `pub enum EventCategory`
- `impl EventCategory`
  - `pub const fn label(self) -> &'static str`
  - `pub const fn dispatchable(self) -> bool`
- `pub fn category(event_type: Option<&str>) -> EventCategory`
- `pub struct MessageEvent`
- `impl MessageEvent`
  - `pub fn is_private(&self) -> bool`
  - `pub fn to_incoming(&self) -> Incoming`
  - `pub fn dedup_key(&self) -> (&str, &str)`
  - `pub fn parse(event: &ProtocolEvent) -> Option<Self>`
- `pub fn mention_tag(scene: Scene, user: &str) -> String`
- `pub fn filter_at(raw: &str) -> String`
- `pub fn strip_mentions(content: &str, data: &Value) -> String`
- `pub fn strip_leading_mention(content: &str) -> String`
- `pub fn mentions_bot(data: &Value) -> bool`

## `src/images.rs`

- `pub fn current_target() -> Target`
- `pub struct HostImageStore`
- `impl HostImageStore`
  - `pub fn new(host: Arc<ImageHost>) -> Self`
  - `pub fn host(&self) -> &Arc<ImageHost>`

## `src/interaction.rs`

- `pub const INTERACTION_TYPE_BUTTON: i64 = 11`
- `pub struct InteractionEvent`
- `impl InteractionEvent`
  - `pub fn parse(event: &ProtocolEvent) -> Option<Self>`
  - `pub fn scene_of(chat_type: Option<i64>, scene: Option<&str>) -> Option<Scene>`

## `src/jobs.rs`

- `pub const JOB_TICK: Duration = Duration::from_secs(1)`
- `pub const JOBS_MODULE: &str = "jobs"`
- `pub struct JobRunner`
- `impl JobRunner`
  - `pub fn new( …`
  - `pub fn registry(&self) -> &Arc<JobRegistry>`
  - `pub fn spawn(self, mut stop: watch::Receiver<bool>) -> JoinHandle<()>`
- `pub struct AdminJobs`
- `impl AdminJobs`
  - `pub fn new(registry: Arc<JobRegistry>) -> Self`
  - `pub fn registry(&self) -> &Arc<JobRegistry>`

## `src/lib.rs`

- `pub use bot::{ wait_for_shutdown_signal, Bot, BotBuilder, ControlCommand, FallbackPolicy, Runtime, RuntimeHandle, ShutdownReport, DEFAULT_EVENT_BUFFER, DEFAULT_SHUTDOWN_TIMEOUT, MODULE, SUPERVISED_ENV, UNSUPERVISED_RESTART_ERROR, }`
- `pub use config::{ BotConfig, DEFAULT_ADMIN_LISTEN, DEFAULT_ADMIN_PORT, DEFAULT_LOG_LEVEL, DEFAULT_PROTOCOL, DEFAULT_PROXY_API, DEFAULT_RENDER_POOL, LOG_LEVELS, }`
- `pub use error::{BotError, FALLBACK_HINT}`
- `pub use event::{mention_tag, EventCategory, MessageEvent, MESSAGE_EVENTS}`
- `pub use lock::{ holder_pid, process_alive, InstanceLock, DEFAULT_LOCK_FILE, INSTANCE_CONFLICT, LOCK_FILE_ENV, }`
- `pub use notice::{avatar_url, compose, mention, pick_index, pick_line, NoticeEvent, NoticeKind}`
- `pub use panels::ApiPanelRemote`
- `pub use pipeline::{ classify_send_error, Outcome, Pipeline, PipelineCounters, PipelineDeps, ProcessReport, PushReport, CARD_REPLY_KINDS, }`
- `pub use plugin_config::PluginConfigStore`
- `pub use ports::{ ApiTokens, CardView, ClosurePluginConfig, DbStats, InboxArchive, InboxBatch, InboxMessage, KvBackend, MemoryInbox, MemoryKv, MemoryStats, PluginConfigurable, RejectedTokens, Rendered, Renderer, ReplyTransport, SchedulerRenderer, StatsDelta, StatsRecorder, }`
- `pub use aurorix_core::{RoleRequired, Scene, Target}`
- `pub use aurorix_gateway::{ EventSink, Frame, FrameIo, GatewayClient, GatewayState, ProtocolEvent, TokenSource, Transport, WebSocketTransport, }`
- `pub use aurorix_log::{Level, Logger, Options}`
- `pub use aurorix_plugin::{ AccessBook, CommandRegistry, CommandSpec, Incoming, Outgoing, ReplyKind, ReplySink, Router, Stage, }`
- `pub use aurorix_render::{RenderError, RenderScheduler, SchedulerConfig}`
- `pub use images::{current_target, with_target, HostImageStore}`
- `pub use interaction::{InteractionEvent, INTERACTION_TYPE_BUTTON}`
- `pub use webhook::{ensure_webhook_intents, webhook_events, WebhookIngress, WEBHOOK_MODE_LABEL}`

## `src/lock.rs`

- `pub const LOCK_FILE_ENV: &str = "AURORIX_LOCK_FILE"`
- `pub const DEFAULT_LOCK_FILE: &str = "/tmp/aurorix.lock"`
- `pub const INSTANCE_CONFLICT: &str = "已有另一个 Aurorix 实例在运行"`
- `pub struct InstanceLock`
- `impl InstanceLock`
  - `pub fn acquire(path: impl AsRef<Path>) -> Result<Self, BotError>`
  - `pub fn path(&self) -> &Path`
  - `pub fn release(&self)`
- `pub fn holder_pid(path: impl AsRef<Path>) -> Option<u32>`
- `pub fn process_alive(pid: u32) -> bool`

## `src/notice.rs`

- `pub const AVATAR_SIZE: usize = 140`
- `pub const AVATAR_SHOW_PX: usize = 40`
- `pub const NO_PERMISSION_CODE: i32 = 40034105`
- `pub const AVATAR_BASE: &str = "https://thirdqq.qlogo.cn/qqapp"`
- `pub const GROUP_MEMBER_ADD: &str = "GROUP_MEMBER_ADD"`
- `pub const GROUP_MEMBER_REMOVE: &str = "GROUP_MEMBER_REMOVE"`
- `pub const GUILD_MEMBER_ADD: &str = "GUILD_MEMBER_ADD"`
- `pub const GUILD_MEMBER_REMOVE: &str = "GUILD_MEMBER_REMOVE"`
- `pub const GROUP_MSG_RECEIVE: &str = "GROUP_MSG_RECEIVE"`
- `pub const GROUP_MSG_REJECT: &str = "GROUP_MSG_REJECT"`
- `pub const C2C_MSG_RECEIVE: &str = "C2C_MSG_RECEIVE"`
- `pub const C2C_MSG_REJECT: &str = "C2C_MSG_REJECT"`
- `pub const MEMBER_EVENTS: [&str; 4] = [`
- `pub const FRIEND_EVENTS: [&str; 2] = [crate::event::FRIEND_ADD, crate::event::FRIEND_DEL]`
- `pub enum NoticeKind`
- `impl NoticeKind`
  - `pub fn of(event_type: &str) -> Option<Self>`
  - `pub const fn added(self) -> bool`
  - `pub const fn friend(self) -> bool`
  - `pub const fn label(self) -> &'static str`
- `pub struct NoticeEvent`
- `impl NoticeEvent`
  - `pub fn parse(event: &ProtocolEvent) -> Option<Self>`
  - `pub fn can_reply(&self) -> bool`
- `pub fn avatar_url(app_id: &str, user_id: &str, size: usize) -> Option<String>`
- `pub fn compose(avatar: &str, user_id: &str, added: bool, text: &str, extra: &str) -> String`
- `pub fn mention(user_id: &str) -> String`
- `pub fn pick_line(pool: &[String]) -> String`
- `pub fn pick_index(count: usize) -> usize`
- `pub struct ReceiveGate`
- `pub fn receive_gate(event: &ProtocolEvent) -> Option<ReceiveGate>`

## `src/panels.rs`

- `pub struct ApiPanelRemote`
- `impl ApiPanelRemote`
  - `pub fn new(api: Arc<BotApi>) -> Self`
  - `pub fn api(&self) -> &Arc<BotApi>`

## `src/pipeline.rs`

- `pub const MODULE: &str = "pipeline"`
- `pub const NAME_REFRESH_TTL: Duration = Duration::from_secs(6 * 60 * 60)`
- `pub const CARD_REPLY_KINDS: [ReplyKind; 2] = [ReplyKind::Handler, ReplyKind::ArgumentError]`
- `pub enum Outcome`
- `impl Outcome`
  - `pub const fn label(self) -> &'static str`
- `pub struct ProcessReport`
- `impl ProcessReport`
- `pub struct PipelineCounters`
- `impl Counters`
- `pub struct PipelineDeps`
- `impl PipelineDeps`
  - `pub fn new( …`
- `pub struct Pipeline`
- `impl Pipeline`
  - `pub fn new(deps: PipelineDeps) -> Arc<Self>`
  - `pub fn sender_for(&self, message: &MessageEvent) -> Option<InvocationSender>`
  - `pub fn config(&self) -> &Arc<BotConfig>`
  - `pub fn router(&self) -> &Arc<Router>`
  - `pub fn transport(&self) -> &Arc<dyn ReplyTransport>`
  - `pub fn logger(&self) -> &Arc<Logger>`
  - `pub fn dedup(&self) -> &aurorix_api::DedupWindow`
  - `pub fn counters(&self) -> PipelineCounters`
  - `pub fn has_inbox(&self) -> bool`
  - `pub fn has_stats(&self) -> bool`
  - `pub fn has_kv(&self) -> bool`
  - `pub fn footers(&self) -> Option<&Arc<FooterHub>>`
  - `pub fn has_footers(&self) -> bool`
  - `pub fn has_renderer(&self) -> bool`
  - `pub fn maintenance(&self) -> Option<String>`
  - `pub fn set_maintenance(&self, text: Option<String>)`
- `pub struct PushReport`
- `impl PushReport`
- `pub fn reply_kind_title(kind: ReplyKind) -> &'static str`
- `pub fn classify_send_error(error: &ApiError) -> (&'static str, Level)`
- `pub fn scene_label(scene: Scene) -> &'static str`

## `src/plugin_config.rs`

- `pub const MODULE: &str = "plugin_config"`
- `pub struct PluginConfigStore`
- `impl PluginConfigStore`
  - `pub fn new(path: Option<PathBuf>, values: BTreeMap<String, BTreeMap<String, Value>>) -> Self`
  - `pub fn path(&self) -> Option<&Path>`
  - `pub fn attach(&self, sink: Arc<dyn PluginConfigurable>)`
  - `pub fn sinks(&self) -> Vec<Arc<dyn PluginConfigurable>>`
  - `pub fn values(&self, plugin_id: &str) -> BTreeMap<String, Value>`
  - `pub fn stored(&self) -> BTreeMap<String, BTreeMap<String, Value>>`
  - `pub fn apply(&self, plugin_id: &str, values: &BTreeMap<String, Value>) -> Vec<String>`
  - `pub fn save(&self, plugin_id: &str, values: BTreeMap<String, Value>) -> Result<(), String>`
  - `pub fn apply_all(&self) -> Vec<String>`

## `src/ports.rs`

- `pub const IMAGE_UPLOAD_UNSUPPORTED: &str = "当前发送通道未实现图片上传, 渲染图发不出去"`
- `pub struct TransportInboxSender`
- `impl TransportInboxSender`
  - `pub fn new(transport: Arc<dyn ReplyTransport>) -> Self`
  - `pub fn transport(&self) -> &Arc<dyn ReplyTransport>`
- `pub trait ReplyTransport: Send + Sync`
- `pub const IMAGE_BYTES_UNSUPPORTED: &str = "该传输不支持频道图片字节直传"`
- `pub struct ApiTokens`
- `impl ApiTokens`
  - `pub fn new(api: Arc<BotApi>) -> Self`
- `pub struct RejectedTokens`
- `impl RejectedTokens`
  - `pub fn new(reason: impl Into<String>) -> Self`
- `pub trait KvBackend: Send + Sync`
- `pub struct MemoryKv`
- `impl MemoryKv`
  - `pub fn new() -> Self`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
- `pub struct InboxMessage`
- `impl InboxMessage`
  - `pub fn inbound( …`
  - `pub fn outbound( …`
- `pub struct InboxBatch`
- `impl InboxBatch`
  - `pub fn new(messages: Vec<InboxMessage>) -> Self`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn push(&mut self, message: InboxMessage)`
- `pub trait InboxArchive: Send + Sync`
- `pub struct MemoryInbox`
- `impl MemoryInbox`
  - `pub fn new() -> Self`
  - `pub fn saved(&self) -> Vec<InboxBatch>`
  - `pub fn messages(&self) -> Vec<InboxMessage>`
  - `pub fn names(&self) -> std::collections::BTreeMap<String, String>`
- `pub struct StatsDelta`
- `pub trait StatsRecorder: Send + Sync`
- `pub struct DbStats`
- `impl DbStats`
  - `pub fn new(store: Arc<StatsStore>) -> Self`
- `pub struct MemoryStats`
- `impl MemoryStats`
  - `pub fn new() -> Self`
  - `pub fn calls(&self) -> usize`
  - `pub fn deltas(&self) -> Vec<StatsDelta>`
  - `pub fn total_received(&self) -> u64`
  - `pub fn names(&self) -> std::collections::BTreeMap<String, String>`
  - `pub fn total_sent(&self) -> u64`
- `pub struct CardView`
- `impl CardView`
  - `pub fn new(scene: Scene, title: impl Into<String>, body: impl Into<String>) -> Self`
  - `pub fn lines(&self) -> Vec<String>`
- `pub enum Rendered`
- `pub const CUSTOM_RENDER_UNSUPPORTED: &str = "该渲染器不支持插件自定义绘图"`
- `pub trait Renderer: Send + Sync`
- `pub const CARD_WIDTH: i32 = 720`
- `pub const CARD_FONT_SIZE: u32 = 24`
- `pub const CARD_FILENAME: &str = "card.png"`
- `pub struct SchedulerRenderer`
- `impl SchedulerRenderer`
  - `pub fn new(scheduler: Arc<RenderScheduler>) -> Self`
  - `pub fn with_size(mut self, width: i32, font_size: u32) -> Self`
  - `pub fn scheduler(&self) -> &Arc<RenderScheduler>`
- `pub trait PluginConfigurable: Send + Sync`
- `pub struct ClosurePluginConfig<F>`
- `impl <F> ClosurePluginConfig<F>`
  - `pub fn new(id: impl Into<String>, apply: F) -> Self`

## `src/webhook.rs`

- `pub const MODULE: &str = "webhook"`
- `pub const WEBHOOK_MODE_LABEL: &str = "Webhook 接收模式已启用"`
- `pub struct WebhookIngress`
- `impl WebhookIngress`
  - `pub fn new(app_secret: impl Into<String>, sink: Arc<dyn EventSink>) -> Self`
  - `pub fn path() -> &'static str`
  - `pub fn router(&self) -> Router`
  - `pub fn into_router(self) -> Router`
- `pub fn webhook_events(app_secret: impl Into<String>, sink: Arc<dyn EventSink>) -> WebhookIngress`
- `pub fn ensure_webhook_intents(intents: &[String]) -> Result<(), BotError>`

