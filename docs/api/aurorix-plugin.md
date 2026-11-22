# aurorix-plugin API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/access.rs`

- `pub const DENY_MESSAGE_LIMIT: usize = 300`
- `pub struct Maintenance`
- `impl Maintenance`
  - `pub fn on(message: impl Into<String>) -> Self`
  - `pub fn normalized(&self) -> Self`
  - `pub fn text(&self) -> Option<String>`
- `pub struct Deny`
- `impl Deny`
  - `pub fn silent() -> Self`
  - `pub fn say(message: impl Into<String>) -> Self`
  - `pub fn is_decided(&self) -> bool`
  - `pub fn normalized(&self) -> Self`
- `pub enum AccessMode`
- `impl AccessMode`
  - `pub fn parse(value: &str) -> Option<Self>`
  - `pub fn as_str(self) -> &'static str`
- `pub struct AccessRule`
- `impl AccessRule`
  - `pub fn is_active(&self) -> bool`
  - `pub fn normalized(&self) -> Self`
- `pub struct AccessConfig`
- `impl AccessConfig`
  - `pub fn plugin_maintenance(mut self, message: impl Into<String>) -> Self`
  - `pub fn command_rule(mut self, path: &str, rule: AccessRule) -> Self`
  - `pub fn normalized(&self) -> Self`
- `pub fn canonical_command_path(path: &str) -> String`
- `pub struct AccessQuery`
- `pub enum Verdict`
- `impl Verdict`
  - `pub fn allowed(&self) -> bool`
  - `pub fn is_silent(&self) -> bool`
- `pub fn check_access( …`
- `pub fn command_maintenance_message(command: &Maintenance, plugin: &Maintenance) -> String`
- `pub fn deny_message(command: &Deny, plugin: &Deny) -> Option<String>`
- `pub struct AccessBook`
- `impl AccessBook`
  - `pub fn new() -> Self`
  - `pub fn set(&self, plugin: &str, config: AccessConfig)`
  - `pub fn get(&self, plugin: &str) -> Option<AccessConfig>`
  - `pub fn disabled(&self, plugin: &str) -> bool`
  - `pub fn plugin_maintenance(&self, plugin: &str) -> Maintenance`
  - `pub fn check(&self, query: &AccessQuery, inline: Option<&AccessRule>) -> Verdict`
- `pub type SharedAccessBook = Arc<AccessBook>`

## `src/args.rs`

- `pub enum ArgKind`
- `pub struct FlagSpec`
- `impl FlagSpec`
  - `pub fn value(name: impl Into<String>) -> Self`
  - `pub fn switch(name: impl Into<String>) -> Self`
  - `pub fn short(mut self, short: char) -> Self`
  - `pub fn help(mut self, help: impl Into<String>) -> Self`
  - `pub fn default_value(mut self, value: impl Into<String>) -> Self`
  - `pub fn summary(&self) -> String`
- `pub struct PositionalSpec`
- `impl PositionalSpec`
  - `pub fn required(name: impl Into<String>) -> Self`
  - `pub fn optional(name: impl Into<String>) -> Self`
  - `pub fn variadic(mut self) -> Self`
  - `pub fn help(mut self, help: impl Into<String>) -> Self`
  - `pub fn summary(&self) -> String`
- `pub struct ArgsSpec`
- `impl ArgsSpec`
  - `pub fn new() -> Self`
  - `pub fn positional(mut self, spec: PositionalSpec) -> Self`
  - `pub fn flag(mut self, flag: FlagSpec) -> Self`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn usage(&self, name: &str) -> String`
  - `pub fn find_long(&self, name: &str) -> Option<&FlagSpec>`
  - `pub fn find_short(&self, short: char) -> Option<&FlagSpec>`
- `pub struct ParsedArgs`
- `impl ParsedArgs`
  - `pub fn from_tokens(tokens: &[String]) -> Self`
  - `pub fn flag(&self, name: &str) -> Option<&str>`
  - `pub fn switch(&self, name: &str) -> bool`
  - `pub fn value(&self, name: &str) -> Option<&str>`
  - `pub fn positional_value(&self, name: &str) -> Option<&str>`
  - `pub fn first(&self) -> Option<&str>`
  - `pub fn is_empty(&self) -> bool`
- `pub struct ArgError`
- `impl ArgError`
  - `pub fn new(message: impl Into<String>, usage: impl Into<String>) -> Self`
- `pub fn parse_args(name: &str, spec: &ArgsSpec, input: &str) -> Result<ParsedArgs, ArgError>`
- `pub fn parse_tokens( …`

## `src/assets.rs`

- `pub const DEFAULT_MAX_ASSET_BYTES: u64 = 32 * 1024 * 1024`
- `pub const FONTS_DIR: &str = "fonts"`
- `pub const TEMPLATES_DIR: &str = "templates"`
- `pub const FONT_SUFFIXES: [&str; 3] = ["ttf", "otf", "ttc"]`
- `pub const TEMPLATE_SUFFIX: &str = "md"`
- `pub enum AssetPathError`
- `pub struct PluginAssetsReport`
- `impl PluginAssetsReport`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn registered(&self) -> usize`
- `pub struct PluginAssets`
- `impl PluginAssets`
  - `pub fn new(plugin_id: impl Into<String>, root: impl Into<PathBuf>) -> Self`
  - `pub fn with_max_bytes(mut self, max_bytes: u64) -> Self`
  - `pub fn root(&self) -> &Path`
  - `pub fn plugin_id(&self) -> &str`
  - `pub fn max_bytes(&self) -> u64`
  - `pub fn root_ready(&self) -> bool`
  - `pub fn resolve(&self, key: &str) -> Result<PathBuf, AssetPathError>`
  - `pub fn read(&self, key: &str) -> Result<Vec<u8>, AssetPathError>`
  - `pub fn exists(&self, key: &str) -> bool`
  - `pub fn font_alias(&self, name: &str) -> Option<String>`
  - `pub fn fonts(&self) -> Vec<String>`
  - `pub fn templates(&self) -> Vec<String>`
  - `pub fn loader(self: &Arc<Self>) -> Arc<dyn AssetLoader>`
  - `pub fn bootstrap(&self, templates: &Arc<Registry>) -> PluginAssetsReport`
  - `pub fn spawn_template_reload( …`
  - `pub fn registry(&self) -> Option<Arc<Registry>>`

## `src/ban.rs`

- `pub const DEFAULT_BAN_MESSAGE: &str = "您的账号因违反Bot使用协议已被封禁"`
- `pub const BAN_MESSAGE_LIMIT: usize = 2000`
- `pub const BAN_THROTTLE: Duration = Duration::from_secs(15)`
- `pub struct BanEntry`
- `impl BanEntry`
  - `pub fn new(openid: impl Into<String>) -> Self`
  - `pub fn with_message(mut self, message: impl Into<String>) -> Self`
  - `pub fn with_note(mut self, note: impl Into<String>) -> Self`
  - `pub fn with_at(mut self, at: impl Into<String>) -> Self`
  - `pub fn text(&self) -> String`
- `pub struct BanList`
- `impl BanList`
  - `pub fn new() -> Self`
  - `pub fn from_entries(entries: impl IntoIterator<Item = BanEntry>) -> Self`
  - `pub fn load(&self, entries: impl IntoIterator<Item = BanEntry>)`
  - `pub fn put(&self, entry: BanEntry) -> Result<(), PluginError>`
  - `pub fn delete(&self, openid: &str) -> Result<(), PluginError>`
  - `pub fn list(&self) -> Vec<BanEntry>`
  - `pub fn contains(&self, openid: &str) -> bool`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn hit(&self, openid: &str) -> Option<String>`
  - `pub fn hit_at(&self, openid: &str, now: Instant) -> Option<String>`

## `src/card.rs`

- `pub const ERROR_CARD_TEMPLATE: &str = "error_card"`
- `pub const FALLBACK_CARD_TEMPLATE: &str = "Card"`
- `pub const ARGUMENT_ERROR_TITLE: &str = "❌ 指令参数错误"`
- `pub const HANDLER_ERROR_TITLE: &str = "❌ 指令执行失败"`
- `pub const REASON_LABEL: &str = "原因"`
- `pub const USAGE_LABEL: &str = "用法"`
- `pub fn render_card( …`
- `pub fn plain_card(title: &str, fields: &[(String, String)]) -> String`
- `pub fn argument_error( …`
- `pub fn handler_failure(templates: &Registry, namespace: &str, reason: &str) -> Outgoing`

## `src/command.rs`

- `pub const PREFIX_SYMBOLS: [&str; 3] = ["/", "#", "!"]`
- `pub fn normalize_name(name: &str) -> String`
- `pub fn prefix_symbol_of(token: &str, prefixes: &[String]) -> Option<String>`
- `pub type HandlerFuture<'a> =`
- `pub trait CommandHandler: Send + Sync`
- `pub fn async_handler<F, Fut>(handler: F) -> Arc<dyn CommandHandler>`
- `pub fn sync_handler<F>(handler: F) -> Arc<dyn CommandHandler>`
- `pub struct CommandSpec`
- `impl CommandSpec`
  - `pub fn new(name: impl Into<String>) -> Self`
  - `pub fn from_macro(meta: (&'static str, bool), handler: Arc<dyn CommandHandler>) -> Self`
  - `pub fn alias(mut self, alias: impl Into<String>) -> Self`
  - `pub fn aliases(mut self, aliases: impl IntoIterator<Item = String>) -> Self`
  - `pub fn prefix(mut self, prefix: impl Into<String>) -> Self`
  - `pub fn prefixes(mut self, prefixes: impl IntoIterator<Item = String>) -> Self`
  - `pub fn role(mut self, role: RoleRequired) -> Self`
  - `pub fn cooldown(mut self, rate: Rate) -> Self`
  - `pub fn access(mut self, rule: AccessRule) -> Self`
  - `pub fn maintenance(mut self, maintenance: Maintenance) -> Self`
  - `pub fn deny(mut self, deny: Deny) -> Self`
  - `pub fn describe(mut self, help: impl Into<String>) -> Self`
  - `pub fn handler(mut self, handler: Arc<dyn CommandHandler>) -> Self`
  - `pub fn on<F, Fut>(mut self, handler: F) -> Self`
  - `pub fn on_sync<F>(mut self, handler: F) -> Self`
  - `pub fn disable_private(mut self) -> Self`
  - `pub fn no_footer(mut self) -> Self`
  - `pub fn with_args(mut self, args: ArgsSpec) -> Self`
  - `pub fn subcommand(mut self, subcommand: CommandSpec) -> Self`
  - `pub fn subcommands(mut self, subcommands: impl IntoIterator<Item = CommandSpec>) -> Self`
  - `pub fn fallback(mut self, handler: Arc<dyn CommandHandler>) -> Self`
- `pub struct CommandNode`
- `impl CommandNode`
  - `pub fn has_handler(&self) -> bool`
  - `pub fn accepts_prefix(&self, prefix: &str) -> bool`
  - `pub fn child(&self, token: &str) -> Option<Arc<CommandNode>>`
  - `pub fn canonical_path(&self) -> &str`
- `pub fn build_node(spec: CommandSpec, plugin: &str, parent_path: &str) -> Arc<CommandNode>`
- `pub struct PluginCommands`
- `impl PluginCommands`
  - `pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self`
  - `pub fn from_plugin(plugin: &dyn Plugin, commands: Vec<CommandSpec>) -> Self`
  - `pub fn describe(mut self, description: impl Into<String>) -> Self`
  - `pub fn config(mut self, field: PluginConfigField) -> Self`
  - `pub fn with_config(mut self, fields: impl IntoIterator<Item = PluginConfigField>) -> Self`
  - `pub fn command(mut self, command: CommandSpec) -> Self`
  - `pub fn with_commands(mut self, commands: impl IntoIterator<Item = CommandSpec>) -> Self`

## `src/config_field.rs`

- `pub enum PluginConfigKind`
- `impl PluginConfigKind`
  - `pub const ALL: [Self; 6] = [`
  - `pub fn as_str(&self) -> &'static str`
  - `pub fn label(&self) -> &'static str`
  - `pub fn parse(raw: &str) -> Option<Self>`
  - `pub fn coerce(&self, value: &Value) -> Option<Value>`
- `pub struct PluginConfigField`
- `impl PluginConfigField`
  - `pub fn text(key: impl Into<String>, label: impl Into<String>) -> Self`
  - `pub fn textarea(key: impl Into<String>, label: impl Into<String>) -> Self`
  - `pub fn number(key: impl Into<String>, label: impl Into<String>) -> Self`
  - `pub fn boolean(key: impl Into<String>, label: impl Into<String>) -> Self`
  - `pub fn password(key: impl Into<String>, label: impl Into<String>) -> Self`
  - `pub fn select( …`
  - `pub fn default_value(mut self, value: impl Into<Value>) -> Self`
  - `pub fn description(mut self, text: impl Into<String>) -> Self`
  - `pub fn placeholder(mut self, text: impl Into<String>) -> Self`
  - `pub fn required(mut self) -> Self`
  - `pub fn check(&self, value: &Value) -> Result<Value, String>`
- `pub fn find_field<'a>(fields: &'a [PluginConfigField], key: &str) -> Option<&'a PluginConfigField>`
- `pub fn validate_values( …`
- `pub fn normalize_values( …`
- `pub fn defaults(fields: &[PluginConfigField]) -> BTreeMap<String, Value>`
- `pub fn resumed_values( …`
- `pub fn fields_json(fields: &[PluginConfigField]) -> Vec<Value>`

## `src/error.rs`

- `pub enum PluginError`
- `impl PluginError`
  - `pub fn handler(message: impl Into<String>) -> Self`
  - `pub fn reply(message: impl Into<String>) -> Self`
- `pub type PluginResult<T> = Result<T, PluginError>`

## `src/help.rs`

- `pub const HELP_HEADER: &str = "### 指令帮助"`
- `pub const HELP_NAME: &str = "help"`
- `pub const HELP_ALIAS: &str = "帮助"`
- `pub const HELP_DESCRIBE: &str = "查看全部指令"`
- `pub fn render_help(registry: &CommandRegistry) -> String`
- `pub fn render_plugin_help(registry: &CommandRegistry, plugin: &str) -> Option<String>`
- `pub fn help_spec(registry: Arc<CommandRegistry>) -> CommandSpec`

## `src/job.rs`

- `pub const JOB_MODULE: &str = "job"`
- `pub enum JobKind`
- `impl JobKind`
  - `pub const fn as_str(self) -> &'static str`
- `pub enum JobSchedule`
- `impl JobSchedule`
  - `pub fn kind(&self) -> JobKind`
  - `pub fn interval(&self) -> Option<Duration>`
  - `pub fn cron(&self) -> Option<&str>`
- `pub struct CronExpr`
- `impl CronExpr`
  - `pub const FIELD_COUNT: usize = 5`
  - `pub fn parse(expr: &str) -> Result<Self, String>`
  - `pub fn next_after(&self, from: DateTime<Local>) -> Option<DateTime<Local>>`
- `pub struct JobRun`
- `pub type JobFuture<'a> =`
- `pub trait JobHandler: Send + Sync`
- `pub fn async_job<F, Fut>(handler: F) -> Arc<dyn JobHandler>`
- `pub fn sync_job<F>(handler: F) -> Arc<dyn JobHandler>`
- `pub struct JobSpec`
- `impl JobSpec`
  - `pub fn interval(id: impl Into<String>, plugin_id: impl Into<String>, every: Duration) -> Self`
  - `pub fn cron( …`
  - `pub fn immediate(mut self, value: bool) -> Self`
  - `pub fn target(mut self, target: Target) -> Self`
- `pub struct JobReport`
- `pub enum JobError`
- `pub struct JobRegistry`
- `impl JobRegistry`
  - `pub fn new() -> Self`
  - `pub fn register(&self, spec: JobSpec, handler: Arc<dyn JobHandler>) -> Result<(), JobError>`
  - `pub fn cancel(&self, id: &str) -> bool`
  - `pub fn pause(&self, id: &str) -> bool`
  - `pub fn resume(&self, id: &str) -> bool`
  - `pub fn set_paused(&self, id: &str, paused: bool) -> bool`
  - `pub fn exists(&self, id: &str) -> bool`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn reports(&self, now: DateTime<Utc>) -> Vec<JobReport>`
  - `pub fn due(&self, now: DateTime<Utc>) -> Vec<(JobSpec, Arc<dyn JobHandler>)>`
  - `pub fn record_fire(&self, id: &str, fired_at: DateTime<Utc>)`
  - `pub fn record_failure(&self, id: &str)`

## `src/lib.rs`

- `pub use aurorix_db::{PluginDb, PluginDbRegistry}`
- `pub use access::{ canonical_command_path, check_access, AccessBook, AccessConfig, AccessMode, AccessQuery, AccessRule, Deny, Maintenance, Verdict, DENY_MESSAGE_LIMIT, }`
- `pub use args::{ parse_args, parse_tokens, tokenize, ArgError, ArgKind, ArgsSpec, FlagSpec, ParsedArgs, PositionalSpec, }`
- `pub use assets::{ AssetPathError, PluginAssets, PluginAssetsReport, DEFAULT_MAX_ASSET_BYTES, FONTS_DIR, FONT_SUFFIXES, TEMPLATES_DIR, TEMPLATE_SUFFIX, }`
- `pub use ban::{BanEntry, BanList, BAN_MESSAGE_LIMIT, BAN_THROTTLE, DEFAULT_BAN_MESSAGE}`
- `pub use card::{argument_error, handler_failure, render_card, ARGUMENT_ERROR_TITLE}`
- `pub use command::{ async_handler, build_node, normalize_name, sync_handler, CommandHandler, CommandNode, CommandSpec, HandlerFuture, PluginCommands, PREFIX_SYMBOLS, }`
- `pub use config_field::{ defaults, fields_json, find_field, normalize_values, resumed_values, validate_values, PluginConfigField, PluginConfigKind, }`
- `pub use error::{PluginError, PluginResult}`
- `pub use help::{help_spec, render_help, render_plugin_help, HELP_HEADER}`
- `pub use job::{ async_job, sync_job, CronExpr, JobError, JobFuture, JobHandler, JobKind, JobRegistry, JobReport, JobRun, JobSchedule, JobSpec, JOB_MODULE, }`
- `pub use rate::{Rate, RateKey, RateLimiter}`
- `pub use registry::{CommandRegistry, PluginSlot, RegistryIndex, RootMatch}`
- `pub use reply::{ Attachment, Incoming, IncomingAttachment, Invocation, InvocationSender, Outgoing, Painter, RenderCard, ReplyKind, ReplySink, SendFuture, }`
- `pub use roles::{ shared, FixedRoleResolver, PlatformRoleResolver, RoleResolver, StaticRoleResolver, }`
- `pub use router::{ role_allows, split_fields, usage_text, DispatchOutcome, PluginAssetBook, Router, Stage, }`
- `pub use aurorix_core::{RoleRequired, Scene, Target}`

## `src/rate.rs`

- `pub const RATE_IDLE_TTL: Duration = Duration::from_secs(600)`
- `pub const RATE_SWEEP_EACH: Duration = Duration::from_secs(60)`
- `pub struct Rate`
- `impl Rate`
  - `pub fn new(limit: usize, window: Duration) -> Self`
  - `pub fn per_window(limit: usize, seconds: u64) -> Self`
  - `pub fn enabled(&self) -> bool`
- `pub struct RateKey`
- `impl RateKey`
  - `pub fn new( …`
- `impl Bucket`
- `pub struct RateLimiter`
- `impl RateLimiter`
  - `pub fn new() -> Self`
  - `pub fn with_capacity(max_buckets: usize) -> Self`
  - `pub fn max_buckets(&self) -> usize`
  - `pub fn check(&self, key: RateKey, rate: Rate) -> Result<(), Duration>`
  - `pub fn check_at(&self, key: RateKey, rate: Rate, now: Instant) -> Result<(), Duration>`
  - `pub fn wait_seconds(wait: Duration) -> u64`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn clear(&self)`

## `src/registry.rs`

- `pub struct PluginSlot`
- `pub struct RegistryIndex`
- `impl RegistryIndex`
  - `pub fn lookup(&self, name: &str) -> Option<Arc<CommandNode>>`
  - `pub fn names(&self) -> Vec<String>`
  - `pub fn command_count(&self) -> usize`
  - `pub fn alias_count(&self) -> usize`
  - `pub fn plugin(&self, id: &str) -> Option<PluginSlot>`
  - `pub fn plugins(&self) -> Vec<PluginSlot>`
  - `pub fn command_paths(&self, plugin: &str) -> BTreeSet<String>`
- `pub struct RootMatch`
- `pub struct SubMatch`
- `pub struct CommandRegistry`
- `impl CommandRegistry`
  - `pub fn new() -> Self`
  - `pub fn with_prefixes(prefixes: Arc<PrefixBook>) -> Self`
  - `pub fn prefix_book(&self) -> Arc<PrefixBook>`
  - `pub fn prefix_chars(&self) -> Vec<String>`
  - `pub fn has_bare(&self) -> bool`
  - `pub fn display_prefix(&self) -> String`
  - `pub fn index(&self) -> Arc<RegistryIndex>`
  - `pub fn names(&self) -> Vec<String>`
  - `pub fn command_count(&self) -> usize`
  - `pub fn plugin(&self, id: &str) -> Option<PluginSlot>`
  - `pub fn plugins(&self) -> Vec<PluginSlot>`
  - `pub fn command_paths(&self, plugin: &str) -> BTreeSet<String>`
  - `pub fn register_plugin(&self, plugin: PluginCommands) -> Vec<String>`
  - `pub fn register(&self, plugin_id: &str, spec: CommandSpec) -> Vec<String>`
  - `pub fn get(&self, name: &str) -> Option<Arc<CommandNode>>`
  - `pub fn match_command(&self, token: &str) -> Option<Arc<CommandNode>>`
  - `pub fn resolve_root(&self, tokens: &[String]) -> Option<RootMatch>`
  - `pub fn resolve( …`

## `src/reply.rs`

- `pub enum ReplyKind`
- `pub struct Attachment`
- `impl Attachment`
  - `pub fn new( …`
  - `pub fn png(data: impl Into<Vec<u8>>) -> Self`
  - `pub fn jpeg(data: impl Into<Vec<u8>>) -> Self`
  - `pub fn webp(data: impl Into<Vec<u8>>) -> Self`
  - `pub fn is_empty(&self) -> bool`
- `pub type Painter = Arc<dyn Fn(&aurorix_render::CanvasKit<'_>) + Send + Sync>`
- `pub struct RenderCard`
- `impl RenderCard`
  - `pub fn new<F>( …`
- `pub struct IncomingAttachment`
- `impl IncomingAttachment`
  - `pub fn new(url: impl Into<String>, content_type: impl Into<String>) -> Self`
  - `pub fn image(&self) -> bool`
  - `pub fn is_empty(&self) -> bool`
- `pub struct Outgoing`
- `impl Outgoing`
  - `pub fn new(kind: ReplyKind, text: impl Into<String>) -> Self`
  - `pub fn at(mut self, at: bool) -> Self`
  - `pub fn reply_to(mut self, message_id: impl Into<String>) -> Self`
  - `pub fn mention_user(mut self, user_id: impl Into<String>) -> Self`
  - `pub fn no_footer(mut self) -> Self`
  - `pub fn image(mut self, attachment: Attachment) -> Self`
  - `pub fn render(mut self, card: RenderCard) -> Self`
  - `pub fn keyboard(mut self, keyboard: aurorix_buttons::Keyboard) -> Self`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn has_render(&self) -> bool`
  - `pub fn has_attachment(&self) -> bool`
- `pub type SendFuture = Pin<Box<dyn Future<Output = Result<(), PluginError>> + Send>>`
- `pub struct InvocationSender(Arc<SendFn>)`
- `impl InvocationSender`
  - `pub fn new<F>(send: F) -> Self`
- `impl Invocation`
  - `pub fn with_plugin_db(mut self, plugin_db: Arc<PluginDb>) -> Self`
  - `pub fn db(&self) -> Option<&Arc<PluginDb>>`
  - `pub fn plugin_db(&self) -> Option<Arc<PluginDb>>`
  - `pub fn has_plugin_db(&self) -> bool`
  - `pub fn kv(&self) -> Option<&aurorix_db::KvStore>`
  - `pub fn with_plugin_assets(mut self, assets: Arc<PluginAssets>) -> Self`
  - `pub fn assets(&self) -> Option<&Arc<PluginAssets>>`
  - `pub fn opt_plugin_assets(&self) -> Option<Arc<PluginAssets>>`
  - `pub fn has_plugin_assets(&self) -> bool`
- `impl Invocation`
  - `pub fn from_event(event: &Incoming) -> Self`
  - `pub fn with_sender(mut self, sender: InvocationSender) -> Self`
  - `pub fn has_sender(&self) -> bool`
  - `pub fn attachments(&self) -> &[IncomingAttachment]`
  - `pub fn images(&self) -> Vec<&IncomingAttachment>`
  - `pub fn quoted(&self) -> Option<&str>`
  - `pub fn has_attachments(&self) -> bool`
  - `pub fn mention_target(&self) -> Option<&str>`
- `pub struct Incoming`
- `impl Incoming`
  - `pub fn new( …`
  - `pub fn with_attachments(mut self, attachments: Vec<IncomingAttachment>) -> Self`
  - `pub fn with_reference(mut self, reference: impl Into<String>) -> Self`
  - `pub fn images(&self) -> Vec<&IncomingAttachment>`
  - `pub fn from_context(context: &aurorix_sdk::Context, content: impl Into<String>) -> Self`
  - `pub fn with_username(mut self, username: impl Into<String>) -> Self`
  - `pub fn with_platform_role(mut self, role: RoleRequired) -> Self`
  - `pub fn as_private(mut self, private: bool) -> Self`
- `pub trait ReplySink: Send + Sync`

## `src/roles.rs`

- `pub trait RoleResolver: Send + Sync`
- `pub struct PlatformRoleResolver`
- `pub struct StaticRoleResolver`
- `impl StaticRoleResolver`
  - `pub fn new() -> Self`
  - `pub fn with_user(mut self, user: impl Into<String>, role: RoleRequired) -> Self`
  - `pub fn with_group(mut self, group: impl Into<String>, role: RoleRequired) -> Self`
  - `pub fn set_user(&mut self, user: impl Into<String>, role: RoleRequired)`
  - `pub fn shared(self) -> Arc<dyn RoleResolver>`
- `pub struct FixedRoleResolver(pub Option<RoleRequired>)`
- `pub fn shared(resolver: impl RoleResolver + 'static) -> Arc<dyn RoleResolver>`

## `src/router.rs`

- `pub enum Stage`
- `impl Stage`
  - `pub const fn level(self) -> u8`
  - `pub const fn label(self) -> &'static str`
- `pub struct DispatchOutcome`
- `impl DispatchOutcome`
  - `pub fn silent(&self) -> bool`
  - `pub fn text(&self) -> Option<&str>`
- `pub struct PluginAssetBook`
- `impl PluginAssetBook`
  - `pub fn new() -> Self`
  - `pub fn with(mut self, plugin_id: impl Into<String>, assets: Arc<PluginAssets>) -> Self`
  - `pub fn insert(&mut self, plugin_id: impl Into<String>, assets: Arc<PluginAssets>)`
  - `pub fn get(&self, plugin_id: &str) -> Option<Arc<PluginAssets>>`
  - `pub fn contains(&self, plugin_id: &str) -> bool`
  - `pub fn names(&self) -> Vec<&str>`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
- `pub struct Router`
- `impl Router`
  - `pub fn new(registry: Arc<CommandRegistry>) -> Self`
  - `pub fn with_access(mut self, access: Arc<AccessBook>) -> Self`
  - `pub fn with_ban(mut self, ban: Arc<BanList>) -> Self`
  - `pub fn with_roles(mut self, roles: Arc<dyn RoleResolver>) -> Self`
  - `pub fn with_limiter(mut self, limiter: Arc<RateLimiter>) -> Self`
  - `pub fn with_templates(mut self, templates: Arc<Registry>) -> Self`
  - `pub fn with_plugin_db_registry(mut self, registry: Arc<aurorix_db::PluginDbRegistry>) -> Self`
  - `pub fn with_plugin_assets(mut self, book: Arc<PluginAssetBook>) -> Self`
  - `pub fn registry(&self) -> &Arc<CommandRegistry>`
  - `pub fn access(&self) -> &Arc<AccessBook>`
  - `pub fn ban(&self) -> &Arc<BanList>`
  - `pub fn limiter(&self) -> &Arc<RateLimiter>`
  - `pub fn set_global_maintenance(&self, maintenance: Maintenance)`
  - `pub fn global_maintenance(&self) -> Maintenance`
- `pub fn split_fields(content: &str) -> Vec<String>`
- `pub fn role_allows(required: Option<RoleRequired>, role: Option<RoleRequired>) -> bool`
- `pub fn usage_text(registry: &CommandRegistry, path: &str) -> String`
- `pub fn default_maintenance_text() -> &'static str`
- `pub fn raw_args( …`

