# aurorix-log API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/context.rs`

- `pub const CONTEXT_PATTERN: &str = r"\b(group|user)=([^\s,;]+)"`
- `pub fn extract_context(msg: &str) -> (String, String)`

## `src/error.rs`

- `pub enum LogError`

## `src/file.rs`

- `pub const DEFAULT_FILE_PREFIX: &str = "aurorix"`
- `pub const DEFAULT_ERROR_PREFIX: &str = "aurorix-error"`
- `pub const DEFAULT_RETENTION_DAYS: u32 = 7`
- `pub const LOG_SUFFIX: &str = ".log"`
- `pub const DATE_PATTERN: &str = "%Y-%m-%d"`
- `pub const DATE_LEN: usize = 10`
- `pub trait Clock: Send + Sync`
- `pub struct SystemClock`
- `pub struct FileOptions`
- `impl FileOptions`
  - `pub fn new(dir: impl Into<PathBuf>) -> Self`
  - `pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self`
  - `pub fn with_error_prefix(mut self, prefix: impl Into<String>) -> Self`
  - `pub fn with_retention_days(mut self, retention_days: u32) -> Self`
  - `pub fn with_min_level(mut self, min_level: Level) -> Self`
  - `pub fn main_path(&self, date: NaiveDate) -> PathBuf`
  - `pub fn error_path(&self, date: NaiveDate) -> PathBuf`
- `pub struct FileSinkStats`
- `pub struct FileSink`
- `impl FileSink`
  - `pub fn open(options: FileOptions) -> Result<Self, LogError>`
  - `pub fn open_with(options: FileOptions, clock: Arc<dyn Clock>) -> Result<Self, LogError>`
  - `pub fn options(&self) -> &FileOptions`
  - `pub fn clock(&self) -> &Arc<dyn Clock>`
  - `pub fn write(&self, record: &LogRecord) -> Result<(), LogError>`
  - `pub fn flush(&self) -> Result<(), LogError>`
  - `pub fn current_paths(&self) -> (PathBuf, PathBuf)`
  - `pub fn current_day(&self) -> Option<NaiveDate>`
  - `pub fn prune(&self) -> Result<usize, LogError>`
  - `pub fn stats(&self) -> FileSinkStats`
  - `pub fn date_of(&self, path: &Path) -> Option<NaiveDate>`

## `src/format.rs`

- `pub const SEPARATOR: &str = " | "`
- `pub const SLOW_MARK: &str = "[SLOW]"`
- `pub const LEVEL_WIDTH: usize = 5`
- `pub const ANSI_RESET: &str = "\x1b[0m"`
- `pub const ANSI_REVERSE: &str = "\x1b[7m"`
- `pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.3f"`
- `pub const NO_COLOR_ENV: &str = "NO_COLOR"`
- `pub enum ColorMode`
- `impl ColorMode`
  - `pub fn resolve(self) -> bool`
- `pub struct FormatOptions`
- `impl FormatOptions`
  - `pub fn plain() -> Self`
  - `pub fn colored() -> Self`
  - `pub fn with_color(mut self, color: ColorMode) -> Self`
  - `pub fn with_slow(mut self, slow: SlowThresholds) -> Self`
  - `pub fn ansi_enabled(&self) -> bool`
- `impl LogRecord`
  - `pub fn render(&self, options: &FormatOptions) -> String`
  - `pub fn render_ansi(&self, options: &FormatOptions) -> String`
- `pub fn format_timestamp(time_ms: i64) -> String`
- `pub fn level_cell(level: Level) -> &'static str`
- `pub fn scene_cell(group: &str, user: &str) -> Option<String>`

## `src/level.rs`

- `pub enum Level`
- `pub const LEVELS: [Level; 4] = [Level::Debug, Level::Info, Level::Warn, Level::Error]`
- `impl Level`
  - `pub fn parse(text: &str) -> Result<Self, LogError>`
  - `pub fn as_str(self) -> &'static str`
  - `pub fn label(self) -> &'static str`
  - `pub fn rank(self) -> u8`
  - `pub fn enables(self, level: Level) -> bool`

## `src/lib.rs`

- `pub use context::{extract_context, CONTEXT_PATTERN}`
- `pub use error::LogError`
- `pub use file::{ Clock, FileOptions, FileSink, FileSinkStats, SystemClock, DATE_PATTERN, DEFAULT_ERROR_PREFIX, DEFAULT_FILE_PREFIX, DEFAULT_RETENTION_DAYS, LOG_SUFFIX, }`
- `pub use format::{ format_timestamp, level_cell, scene_cell, ColorMode, FormatOptions, ANSI_RESET, ANSI_REVERSE, LEVEL_WIDTH, NO_COLOR_ENV, SEPARATOR, SLOW_MARK, TIMESTAMP_FORMAT, }`
- `pub use level::{Level, LEVELS}`
- `pub use logger::{ current_trace, global, init, module, query, recent, set_level, set_level_text, stats, subscribe, summary, with_trace, Logger, ModuleHandle, Options, SUBSCRIBER_CAPACITY, }`
- `pub use record::{truncate_message, LogRecord, DURATION_BYTES, MAX_MESSAGE_BYTES, TRUNCATION_MARK}`
- `pub use ring::{ PushOutcome, Ring, RingFilter, RingStats, RingSummary, DEFAULT_QUERY_LIMIT, DEFAULT_RING_BYTES, DEFAULT_RING_CAPACITY, }`
- `pub use slow::{ SlowThresholds, DB_MODULE_MARKER, DEFAULT_DB_MS, DEFAULT_RENDER_MS, DEFAULT_UPSTREAM_MS, RENDER_MODULE_MARKER, }`

## `src/logger.rs`

- `pub const SUBSCRIBER_CAPACITY: usize = 128`
- `pub fn current_trace() -> Option<String>`
- `pub struct Options`
- `impl Options`
  - `pub fn with_level(mut self, level: Level) -> Self`
  - `pub fn with_capacity(mut self, capacity: usize) -> Self`
  - `pub fn with_byte_budget(mut self, byte_budget: usize) -> Self`
  - `pub fn with_console(mut self, console: bool) -> Self`
  - `pub fn with_format(mut self, format: FormatOptions) -> Self`
  - `pub fn with_color(mut self, color: ColorMode) -> Self`
  - `pub fn with_slow(mut self, slow: SlowThresholds) -> Self`
  - `pub fn with_file(mut self, file: FileOptions) -> Self`
- `pub struct Logger`
- `impl Logger`
  - `pub fn new(options: Options) -> Arc<Self>`
  - `pub fn level(&self) -> Level`
  - `pub fn set_level(&self, level: Level)`
  - `pub fn set_level_text(&self, text: &str) -> Result<Level, LogError>`
  - `pub fn enabled(&self, level: Level) -> bool`
  - `pub fn ring(&self) -> &Ring`
  - `pub fn format(&self) -> &FormatOptions`
  - `pub fn sink(&self) -> Option<&FileSink>`
  - `pub fn sink_errors(&self) -> u64`
  - `pub fn module(self: &Arc<Self>, name: &str) -> ModuleHandle`
  - `pub fn log(&self, level: Level, module: &str, msg: &str, fields: BTreeMap<String, String>)`
  - `pub fn log_timed( …`
  - `pub fn debug(&self, module: &str, msg: &str)`
  - `pub fn info(&self, module: &str, msg: &str)`
  - `pub fn warn(&self, module: &str, msg: &str)`
  - `pub fn error(&self, module: &str, msg: &str)`
  - `pub fn subscribe(&self) -> broadcast::Receiver<LogRecord>`
  - `pub fn subscribers(&self) -> usize`
  - `pub fn lagged(&self) -> u64`
  - `pub fn recv(&self, receiver: &mut broadcast::Receiver<LogRecord>) -> Option<LogRecord>`
  - `pub fn recent(&self, limit: usize) -> Vec<LogRecord>`
  - `pub fn query(&self, filter: &RingFilter) -> Vec<LogRecord>`
  - `pub fn scopes(&self) -> Vec<String>`
  - `pub fn stats(&self) -> RingStats`
  - `pub fn summary(&self) -> RingSummary`
  - `pub fn total(&self) -> u64`
  - `pub fn errors(&self) -> u64`
  - `pub fn flush(&self)`
- `pub struct ModuleHandle`
- `impl ModuleHandle`
  - `pub fn name(&self) -> &str`
  - `pub fn log(&self, level: Level, msg: &str)`
  - `pub fn log_fields(&self, level: Level, msg: &str, fields: BTreeMap<String, String>)`
  - `pub fn log_timed( …`
  - `pub fn debug(&self, msg: &str)`
  - `pub fn info(&self, msg: &str)`
  - `pub fn warn(&self, msg: &str)`
  - `pub fn error(&self, msg: &str)`
- `pub fn init(options: Options) -> Arc<Logger>`
- `pub fn global() -> Arc<Logger>`
- `pub fn module(name: &str) -> ModuleHandle`
- `pub fn set_level(level: Level)`
- `pub fn set_level_text(text: &str) -> Result<Level, LogError>`
- `pub fn subscribe() -> broadcast::Receiver<LogRecord>`
- `pub fn recent(limit: usize) -> Vec<LogRecord>`
- `pub fn query(filter: &RingFilter) -> Vec<LogRecord>`
- `pub fn stats() -> RingStats`
- `pub fn summary() -> RingSummary`

## `src/record.rs`

- `pub const MAX_MESSAGE_BYTES: usize = 8 << 10`
- `pub const TRUNCATION_MARK: &str = "…(截断)"`
- `pub const RECORD_OVERHEAD_BYTES: usize = 64`
- `pub const DURATION_BYTES: usize = 8`
- `pub struct LogRecord`
- `impl LogRecord`
  - `pub fn new( …`
  - `pub fn with_id(mut self, id: u64) -> Self`
  - `pub fn with_trace(mut self, trace: impl Into<String>) -> Self`
  - `pub fn with_duration(mut self, duration: Duration) -> Self`
  - `pub fn with_duration_ms(mut self, duration_ms: u64) -> Self`
  - `pub fn size(&self) -> usize`
  - `pub fn fitted(mut self) -> Self`
- `pub fn truncate_message(msg: &str) -> String`

## `src/ring.rs`

- `pub const DEFAULT_RING_CAPACITY: usize = 2048`
- `pub const DEFAULT_RING_BYTES: usize = 8 << 20`
- `pub struct RingStats`
- `pub struct PushOutcome`
- `pub struct RingSummary`
- `pub struct Ring`
- `impl Ring`
  - `pub fn new(capacity: usize, byte_budget: usize) -> Self`
  - `pub fn capacity(&self) -> usize`
  - `pub fn byte_budget(&self) -> usize`
  - `pub fn push(&self, record: LogRecord) -> PushOutcome`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn iter_newest_first(&self, limit: usize) -> Vec<LogRecord>`
  - `pub fn stats(&self) -> RingStats`
  - `pub fn summary(&self) -> RingSummary`
  - `pub fn clear(&self)`
- `pub const DEFAULT_QUERY_LIMIT: usize = 500`
- `pub struct RingFilter`
- `impl RingFilter`
  - `pub fn new() -> Self`
  - `pub fn with_min_level(mut self, min_level: Level) -> Self`
  - `pub fn with_module(mut self, module: impl Into<String>) -> Self`
  - `pub fn with_trace(mut self, trace: impl Into<String>) -> Self`
  - `pub fn with_group(mut self, group: impl Into<String>) -> Self`
  - `pub fn with_user(mut self, user: impl Into<String>) -> Self`
  - `pub fn with_keyword(mut self, keyword: impl Into<String>) -> Self`
  - `pub fn with_limit(mut self, limit: usize) -> Self`
  - `pub fn effective_limit(&self) -> usize`
  - `pub fn matches(&self, record: &LogRecord) -> bool`
- `impl Ring`
  - `pub fn query(&self, filter: &RingFilter) -> Vec<LogRecord>`
  - `pub fn scopes(&self) -> Vec<String>`

## `src/slow.rs`

- `pub const DEFAULT_DB_MS: u64 = 200`
- `pub const DEFAULT_RENDER_MS: u64 = 1000`
- `pub const DEFAULT_UPSTREAM_MS: u64 = 1500`
- `pub const DB_MODULE_MARKER: &str = "db"`
- `pub const RENDER_MODULE_MARKER: &str = "render"`
- `pub struct SlowThresholds`
- `impl SlowThresholds`
  - `pub fn new(db_ms: u64, render_ms: u64, upstream_ms: u64) -> Self`
  - `pub fn with_db_ms(mut self, db_ms: u64) -> Self`
  - `pub fn with_render_ms(mut self, render_ms: u64) -> Self`
  - `pub fn with_upstream_ms(mut self, upstream_ms: u64) -> Self`
  - `pub fn threshold_ms(&self, module: &str) -> u64`
  - `pub fn is_slow(&self, module: &str, duration_ms: u64) -> bool`
- `impl LogRecord`
  - `pub fn threshold_ms(&self, thresholds: &SlowThresholds) -> u64`
  - `pub fn is_slow(&self, thresholds: &SlowThresholds) -> bool`

