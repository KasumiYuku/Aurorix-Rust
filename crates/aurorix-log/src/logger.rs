use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::broadcast;

use crate::context;
use crate::error::LogError;
use crate::file::{FileOptions, FileSink};
use crate::format::{ColorMode, FormatOptions};
use crate::level::Level;
use crate::record::LogRecord;
use crate::ring::{
    Ring, RingFilter, RingStats, RingSummary, DEFAULT_RING_BYTES, DEFAULT_RING_CAPACITY,
};
use crate::slow::SlowThresholds;

pub const SUBSCRIBER_CAPACITY: usize = 128;

tokio::task_local! {
    static TRACE_ID: String;
}

pub fn current_trace() -> Option<String> {
    TRACE_ID.try_with(|trace| trace.clone()).ok()
}

pub async fn with_trace<F: std::future::Future>(trace: impl Into<String>, future: F) -> F::Output {
    TRACE_ID.scope(trace.into(), future).await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub level: Level,
    pub capacity: usize,
    pub byte_budget: usize,
    pub console: bool,
    pub format: FormatOptions,
    pub file: Option<FileOptions>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            level: Level::Info,
            capacity: DEFAULT_RING_CAPACITY,
            byte_budget: DEFAULT_RING_BYTES,
            console: true,
            format: FormatOptions::default(),
            file: None,
        }
    }
}

impl Options {
    pub fn with_level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity;
        self
    }

    pub fn with_byte_budget(mut self, byte_budget: usize) -> Self {
        self.byte_budget = byte_budget;
        self
    }

    pub fn with_console(mut self, console: bool) -> Self {
        self.console = console;
        self
    }

    pub fn with_format(mut self, format: FormatOptions) -> Self {
        self.format = format;
        self
    }

    pub fn with_color(mut self, color: ColorMode) -> Self {
        self.format.color = color;
        self
    }

    pub fn with_slow(mut self, slow: SlowThresholds) -> Self {
        self.format.slow = slow;
        self
    }

    pub fn with_file(mut self, file: FileOptions) -> Self {
        self.file = Some(file);
        self
    }
}

#[derive(Debug)]
pub struct Logger {
    level: AtomicU8,
    console: bool,
    console_ansi: bool,
    format: FormatOptions,
    ring: Ring,
    sink: Option<FileSink>,
    sender: broadcast::Sender<LogRecord>,
    next_id: AtomicU64,
    errors: AtomicU64,
    lagged: AtomicU64,
    sink_errors: AtomicU64,
}

impl Logger {
    pub fn new(options: Options) -> Arc<Self> {
        let (sender, _receiver) = broadcast::channel(SUBSCRIBER_CAPACITY);
        let mut sink_errors = 0u64;
        let sink = match options.file.clone().map(FileSink::open) {
            Some(Ok(sink)) => Some(sink),
            Some(Err(_)) => {
                sink_errors = 1;
                None
            }
            None => None,
        };
        let console_ansi = options.format.ansi_enabled();
        Arc::new(Self {
            level: AtomicU8::new(options.level.rank()),
            console: options.console,
            console_ansi,
            format: options.format,
            ring: Ring::new(options.capacity, options.byte_budget),
            sink,
            sender,
            next_id: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            lagged: AtomicU64::new(0),
            sink_errors: AtomicU64::new(sink_errors),
        })
    }

    pub fn level(&self) -> Level {
        match self.level.load(Ordering::Relaxed) {
            0 => Level::Debug,
            1 => Level::Info,
            2 => Level::Warn,
            _ => Level::Error,
        }
    }

    pub fn set_level(&self, level: Level) {
        self.level.store(level.rank(), Ordering::Relaxed);
    }

    pub fn set_level_text(&self, text: &str) -> Result<Level, LogError> {
        let level = Level::parse(text)?;
        self.set_level(level);
        Ok(level)
    }

    pub fn enabled(&self, level: Level) -> bool {
        self.level().enables(level)
    }

    pub fn ring(&self) -> &Ring {
        &self.ring
    }

    pub fn format(&self) -> &FormatOptions {
        &self.format
    }

    pub fn sink(&self) -> Option<&FileSink> {
        self.sink.as_ref()
    }

    pub fn sink_errors(&self) -> u64 {
        self.sink_errors.load(Ordering::Relaxed)
    }

    pub fn module(self: &Arc<Self>, name: &str) -> ModuleHandle {
        ModuleHandle {
            logger: Arc::clone(self),
            module: normalize_module(name),
        }
    }

    pub fn log(&self, level: Level, module: &str, msg: &str, fields: BTreeMap<String, String>) {
        self.emit(level, module, msg, fields, None);
    }

    pub fn log_timed(
        &self,
        level: Level,
        module: &str,
        msg: &str,
        fields: BTreeMap<String, String>,
        duration: Duration,
    ) {
        self.emit(level, module, msg, fields, Some(duration));
    }

    fn emit(
        &self,
        level: Level,
        module: &str,
        msg: &str,
        fields: BTreeMap<String, String>,
        duration: Option<Duration>,
    ) {
        if !self.enabled(level) {
            return;
        }
        let module = normalize_module(module);
        let mut record = LogRecord::new(level, module, msg, fields);
        if let Some(duration) = duration {
            record = record.with_duration(duration);
        }
        let (group, user) = context::extract_context(&record.msg);
        record.group = group;
        record.user = user;
        let mut record = record.fitted();
        record.id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        if record.trace.is_none() {
            record.trace = current_trace();
        }
        self.ring.push(record.clone());
        if level == Level::Error {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        self.write_file(&record);
        if self.console {
            self.write_console(&record);
        }
        let _ = self.sender.send(record);
    }

    pub fn debug(&self, module: &str, msg: &str) {
        self.log(Level::Debug, module, msg, BTreeMap::new());
    }

    pub fn info(&self, module: &str, msg: &str) {
        self.log(Level::Info, module, msg, BTreeMap::new());
    }

    pub fn warn(&self, module: &str, msg: &str) {
        self.log(Level::Warn, module, msg, BTreeMap::new());
    }

    pub fn error(&self, module: &str, msg: &str) {
        self.log(Level::Error, module, msg, BTreeMap::new());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogRecord> {
        self.sender.subscribe()
    }

    pub fn subscribers(&self) -> usize {
        self.sender.receiver_count()
    }

    pub fn lagged(&self) -> u64 {
        self.lagged.load(Ordering::Relaxed)
    }

    pub fn recv(&self, receiver: &mut broadcast::Receiver<LogRecord>) -> Option<LogRecord> {
        loop {
            match receiver.try_recv() {
                Ok(record) => return Some(record),
                Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                    self.lagged.fetch_add(skipped, Ordering::Relaxed);
                }
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => return None,
            }
        }
    }

    pub fn recent(&self, limit: usize) -> Vec<LogRecord> {
        self.ring.iter_newest_first(limit)
    }

    pub fn query(&self, filter: &RingFilter) -> Vec<LogRecord> {
        self.ring.query(filter)
    }

    pub fn scopes(&self) -> Vec<String> {
        self.ring.scopes()
    }

    pub fn stats(&self) -> RingStats {
        self.ring.stats()
    }

    pub fn summary(&self) -> RingSummary {
        self.ring.summary()
    }

    pub fn total(&self) -> u64 {
        self.ring.stats().total
    }

    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    pub fn flush(&self) {
        if let Some(sink) = &self.sink {
            if sink.flush().is_err() {
                self.sink_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        aurorix_core::sys::trim_memory();
    }

    fn write_file(&self, record: &LogRecord) {
        let sink = match &self.sink {
            Some(sink) => sink,
            None => return,
        };
        if sink.write(record).is_err() {
            self.sink_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn write_console(&self, record: &LogRecord) {
        let line = if self.console_ansi {
            record.render_ansi(&self.format)
        } else {
            record.render(&self.format)
        };
        let stderr = std::io::stderr();
        let mut guard = stderr.lock();
        let _ = writeln!(guard, "{line}");
    }
}

#[derive(Debug, Clone)]
pub struct ModuleHandle {
    logger: Arc<Logger>,
    module: String,
}

impl ModuleHandle {
    pub fn name(&self) -> &str {
        &self.module
    }

    pub fn log(&self, level: Level, msg: &str) {
        self.logger.log(level, &self.module, msg, BTreeMap::new());
    }

    pub fn log_fields(&self, level: Level, msg: &str, fields: BTreeMap<String, String>) {
        self.logger.log(level, &self.module, msg, fields);
    }

    pub fn log_timed(
        &self,
        level: Level,
        msg: &str,
        fields: BTreeMap<String, String>,
        duration: Duration,
    ) {
        self.logger
            .log_timed(level, &self.module, msg, fields, duration);
    }

    pub fn debug(&self, msg: &str) {
        self.log(Level::Debug, msg);
    }

    pub fn info(&self, msg: &str) {
        self.log(Level::Info, msg);
    }

    pub fn warn(&self, msg: &str) {
        self.log(Level::Warn, msg);
    }

    pub fn error(&self, msg: &str) {
        self.log(Level::Error, msg);
    }
}

fn normalize_module(module: &str) -> String {
    let trimmed = module.trim();
    if trimmed.is_empty() {
        "system".to_string()
    } else {
        trimmed.to_string()
    }
}

static GLOBAL: OnceLock<Arc<Logger>> = OnceLock::new();

pub fn init(options: Options) -> Arc<Logger> {
    GLOBAL.get_or_init(|| Logger::new(options)).clone()
}

pub fn global() -> Arc<Logger> {
    init(Options::default())
}

pub fn module(name: &str) -> ModuleHandle {
    global().module(name)
}

pub fn set_level(level: Level) {
    global().set_level(level);
}

pub fn set_level_text(text: &str) -> Result<Level, LogError> {
    global().set_level_text(text)
}

pub fn subscribe() -> broadcast::Receiver<LogRecord> {
    global().subscribe()
}

pub fn recent(limit: usize) -> Vec<LogRecord> {
    global().recent(limit)
}

pub fn query(filter: &RingFilter) -> Vec<LogRecord> {
    global().query(filter)
}

pub fn stats() -> RingStats {
    global().stats()
}

pub fn summary() -> RingSummary {
    global().summary()
}
