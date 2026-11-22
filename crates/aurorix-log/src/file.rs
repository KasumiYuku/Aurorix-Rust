use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{Days, Local, NaiveDate};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::LogError;
use crate::format::FormatOptions;
use crate::level::Level;
use crate::record::LogRecord;

pub const DEFAULT_FILE_PREFIX: &str = "aurorix";

pub const DEFAULT_ERROR_PREFIX: &str = "aurorix-error";

pub const DEFAULT_RETENTION_DAYS: u32 = 7;

pub const LOG_SUFFIX: &str = ".log";

pub const DATE_PATTERN: &str = "%Y-%m-%d";

pub const DATE_LEN: usize = 10;

pub trait Clock: Send + Sync {
    fn today(&self) -> NaiveDate;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn today(&self) -> NaiveDate {
        Local::now().date_naive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOptions {
    pub dir: PathBuf,
    pub prefix: String,
    pub error_prefix: String,
    pub retention_days: u32,
    pub min_level: Level,
}

impl Default for FileOptions {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("logs"),
            prefix: DEFAULT_FILE_PREFIX.to_string(),
            error_prefix: DEFAULT_ERROR_PREFIX.to_string(),
            retention_days: DEFAULT_RETENTION_DAYS,
            min_level: Level::Info,
        }
    }
}

impl FileOptions {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            ..Self::default()
        }
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn with_error_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.error_prefix = prefix.into();
        self
    }

    pub fn with_retention_days(mut self, retention_days: u32) -> Self {
        self.retention_days = retention_days.max(1);
        self
    }

    pub fn with_min_level(mut self, min_level: Level) -> Self {
        self.min_level = min_level;
        self
    }

    pub fn main_path(&self, date: NaiveDate) -> PathBuf {
        self.dir.join(format!(
            "{}-{}{}",
            self.prefix,
            date.format(DATE_PATTERN),
            LOG_SUFFIX
        ))
    }

    pub fn error_path(&self, date: NaiveDate) -> PathBuf {
        self.dir.join(format!(
            "{}-{}{}",
            self.error_prefix,
            date.format(DATE_PATTERN),
            LOG_SUFFIX
        ))
    }

    fn retention(&self) -> u64 {
        u64::from(self.retention_days.max(1))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSinkStats {
    pub lines: u64,
    pub error_lines: u64,
    pub write_errors: u64,
    pub rotations: u64,
    pub pruned: u64,
}

#[derive(Debug, Default)]
struct State {
    day: Option<NaiveDate>,
    main: Option<File>,
    error: Option<File>,
    lines: u64,
    error_lines: u64,
}

pub struct FileSink {
    options: FileOptions,
    clock: Arc<dyn Clock>,
    format: FormatOptions,
    state: Mutex<State>,
    write_errors: AtomicU64,
    rotations: AtomicU64,
    pruned: AtomicU64,
}

impl fmt::Debug for FileSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileSink")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl FileSink {
    pub fn open(options: FileOptions) -> Result<Self, LogError> {
        Self::open_with(options, Arc::new(SystemClock))
    }

    pub fn open_with(options: FileOptions, clock: Arc<dyn Clock>) -> Result<Self, LogError> {
        let sink = Self {
            options,
            clock,
            format: FormatOptions::plain(),
            state: Mutex::new(State::default()),
            write_errors: AtomicU64::new(0),
            rotations: AtomicU64::new(0),
            pruned: AtomicU64::new(0),
        };
        let today = sink.clock.today();
        {
            let mut state = sink.state.lock();
            sink.rotate(&mut state, today)?;
        }
        Ok(sink)
    }

    pub fn options(&self) -> &FileOptions {
        &self.options
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    pub fn write(&self, record: &LogRecord) -> Result<(), LogError> {
        if !self.options.min_level.enables(record.level) {
            return Ok(());
        }
        let today = self.clock.today();
        let mut state = self.state.lock();
        if state.day != Some(today) {
            self.rotate(&mut state, today)?;
        }
        let line = record.render(&self.format);
        let main = match state.main.as_mut() {
            Some(file) => file,
            None => return Err(self.fail(LogError::Io("日志文件未打开".to_string()))),
        };
        main.write_all(line.as_bytes())
            .and_then(|_| main.write_all(b"\n"))
            .map_err(|err| self.io_error(err))?;
        state.lines += 1;
        if record.level.rank() >= Level::Warn.rank() {
            let error = match state.error.as_mut() {
                Some(file) => file,
                None => return Err(self.fail(LogError::Io("错误日志文件未打开".to_string()))),
            };
            error
                .write_all(line.as_bytes())
                .and_then(|_| error.write_all(b"\n"))
                .map_err(|err| self.io_error(err))?;
            state.error_lines += 1;
        }
        Ok(())
    }

    pub fn flush(&self) -> Result<(), LogError> {
        let mut state = self.state.lock();
        if let Some(main) = state.main.as_mut() {
            main.flush().map_err(|err| self.io_error(err))?;
        }
        if let Some(error) = state.error.as_mut() {
            error.flush().map_err(|err| self.io_error(err))?;
        }
        Ok(())
    }

    pub fn current_paths(&self) -> (PathBuf, PathBuf) {
        let today = self.clock.today();
        let day = {
            let state = self.state.lock();
            state.day.unwrap_or(today)
        };
        (self.options.main_path(day), self.options.error_path(day))
    }

    pub fn current_day(&self) -> Option<NaiveDate> {
        self.state.lock().day
    }

    pub fn prune(&self) -> Result<usize, LogError> {
        let today = self.clock.today();
        let keep = self.options.retention();
        let cutoff = today
            .checked_sub_days(Days::new(keep - 1))
            .ok_or_else(|| LogError::Io("保留天数超出可表示范围".to_string()))?;
        let entries = fs::read_dir(&self.options.dir).map_err(|err| self.io_error(err))?;
        let mut removed = 0usize;
        for entry in entries {
            let entry = entry.map_err(|err| self.io_error(err))?;
            let path = entry.path();
            let is_file = entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false);
            if !is_file {
                continue;
            }
            let date = match self.date_of(&path) {
                Some(date) => date,
                None => continue,
            };
            if date >= today || date >= cutoff {
                continue;
            }
            fs::remove_file(&path).map_err(|err| self.io_error(err))?;
            removed += 1;
        }
        self.pruned.fetch_add(removed as u64, Ordering::Relaxed);
        Ok(removed)
    }

    pub fn stats(&self) -> FileSinkStats {
        let state = self.state.lock();
        FileSinkStats {
            lines: state.lines,
            error_lines: state.error_lines,
            write_errors: self.write_errors.load(Ordering::Relaxed),
            rotations: self.rotations.load(Ordering::Relaxed),
            pruned: self.pruned.load(Ordering::Relaxed),
        }
    }

    pub fn date_of(&self, path: &Path) -> Option<NaiveDate> {
        let name = path.file_name()?.to_str()?;
        let stem = name.strip_suffix(LOG_SUFFIX)?;
        let index = stem.len().checked_sub(DATE_LEN)?;
        let date_text = stem.get(index..)?;
        let head = stem.get(..index)?.strip_suffix('-')?;
        if head != self.options.prefix && head != self.options.error_prefix {
            return None;
        }
        NaiveDate::parse_from_str(date_text, DATE_PATTERN).ok()
    }

    fn rotate(&self, state: &mut State, today: NaiveDate) -> Result<(), LogError> {
        fs::create_dir_all(&self.options.dir).map_err(|err| self.io_error(err))?;
        let main = open_append(&self.options.main_path(today)).map_err(|err| self.io_error(err))?;
        let error =
            open_append(&self.options.error_path(today)).map_err(|err| self.io_error(err))?;
        state.main = Some(main);
        state.error = Some(error);
        state.day = Some(today);
        self.rotations.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn io_error(&self, err: std::io::Error) -> LogError {
        self.fail(LogError::Io(err.to_string()))
    }

    fn fail(&self, err: LogError) -> LogError {
        self.write_errors.fetch_add(1, Ordering::Relaxed);
        err
    }
}

fn open_append(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex as StdMutex;

    use super::*;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    struct FakeClock {
        day: StdMutex<NaiveDate>,
    }

    impl FakeClock {
        fn new(year: i32, month: u32, day: u32) -> Arc<Self> {
            Arc::new(Self {
                day: StdMutex::new(NaiveDate::from_ymd_opt(year, month, day).expect("合法日期")),
            })
        }

        fn set(&self, year: i32, month: u32, day: u32) {
            *self.day.lock().expect("时钟锁") =
                NaiveDate::from_ymd_opt(year, month, day).expect("合法日期");
        }
    }

    impl Clock for FakeClock {
        fn today(&self) -> NaiveDate {
            *self.day.lock().expect("时钟锁")
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aurorix-log-file-{tag}-{}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    fn sink(dir: &Path, clock: Arc<FakeClock>) -> FileSink {
        FileSink::open_with(
            FileOptions::new(dir)
                .with_prefix("app")
                .with_error_prefix("app-err")
                .with_min_level(Level::Debug),
            clock,
        )
        .expect("打开日志文件")
    }

    fn record(level: Level, msg: &str) -> LogRecord {
        let mut record = LogRecord::new(level, "gateway", msg, BTreeMap::new());
        record.time = 1_771_081_445_123;
        record
    }

    #[test]
    fn system_clock_follows_the_local_date() {
        let before = Local::now().date_naive();
        let today = SystemClock.today();
        let after = Local::now().date_naive();
        assert!(today == before || today == after, "{today} 必须是本地日期");
    }

    #[test]
    fn rotation_uses_the_local_calendar_day() {
        let dir = temp_dir("local-day");
        let clock = FakeClock::new(2026, 2, 14);
        let sink = sink(&dir, clock.clone());
        let (main_path, error_path) = sink.current_paths();
        assert_eq!(main_path, dir.join("app-2026-02-14.log"), "按本地日期命名");
        assert_eq!(error_path, dir.join("app-err-2026-02-14.log"));
        sink.write(&record(Level::Error, "第一天")).expect("写入");
        sink.flush().expect("刷新");
        clock.set(2026, 2, 15);
        sink.write(&record(Level::Info, "第二天")).expect("写入");
        sink.flush().expect("刷新");
        let (next_main, next_error) = sink.current_paths();
        assert_eq!(
            next_main,
            dir.join("app-2026-02-15.log"),
            "跨本地日换新文件"
        );
        assert_eq!(next_error, dir.join("app-err-2026-02-15.log"));
        let first_day = fs::read_to_string(&main_path).expect("读旧文件");
        assert_eq!(first_day.lines().count(), 1, "旧文件不再追加");
        let second_day = fs::read_to_string(&next_main).expect("读新文件");
        assert_eq!(second_day.lines().count(), 1, "新文件从零开始");
        assert_eq!(
            sink.current_day(),
            Some(NaiveDate::from_ymd_opt(2026, 2, 15).expect("合法日期"))
        );
        assert_eq!(
            sink.date_of(&main_path),
            Some(NaiveDate::from_ymd_opt(2026, 2, 14).expect("合法日期"))
        );
        assert_eq!(sink.stats().rotations, 2, "打开一次 + 跨天一次");
        fs::remove_dir_all(&dir).expect("清理临时目录");
    }
}
