use std::fmt::Write as _;
use std::io::IsTerminal;

use chrono::{Local, TimeZone};

use crate::level::Level;
use crate::record::LogRecord;
use crate::slow::SlowThresholds;

pub const SEPARATOR: &str = " | ";

pub const SLOW_MARK: &str = "[SLOW]";

pub const LEVEL_WIDTH: usize = 5;

pub const ANSI_RESET: &str = "\x1b[0m";

pub const ANSI_REVERSE: &str = "\x1b[7m";

pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.3f";

pub const NO_COLOR_ENV: &str = "NO_COLOR";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn resolve(self) -> bool {
        match self {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => detect_color(),
        }
    }
}

impl std::fmt::Display for ColorMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            ColorMode::Auto => "auto",
            ColorMode::Always => "always",
            ColorMode::Never => "never",
        })
    }
}

fn detect_color() -> bool {
    if std::env::var_os(NO_COLOR_ENV).is_some() {
        return false;
    }
    std::io::stderr().is_terminal()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatOptions {
    pub color: ColorMode,
    pub slow: SlowThresholds,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            color: ColorMode::Auto,
            slow: SlowThresholds::default(),
        }
    }
}

impl FormatOptions {
    pub fn plain() -> Self {
        Self {
            color: ColorMode::Never,
            slow: SlowThresholds::default(),
        }
    }

    pub fn colored() -> Self {
        Self {
            color: ColorMode::Always,
            slow: SlowThresholds::default(),
        }
    }

    pub fn with_color(mut self, color: ColorMode) -> Self {
        self.color = color;
        self
    }

    pub fn with_slow(mut self, slow: SlowThresholds) -> Self {
        self.slow = slow;
        self
    }

    pub fn ansi_enabled(&self) -> bool {
        self.color.resolve()
    }
}

impl LogRecord {
    pub fn render(&self, options: &FormatOptions) -> String {
        self.render_with(options, false)
    }

    pub fn render_ansi(&self, options: &FormatOptions) -> String {
        self.render_with(options, true)
    }

    fn render_with(&self, options: &FormatOptions, ansi: bool) -> String {
        let mut line = String::with_capacity(160);
        line.push_str(&format_timestamp(self.time));
        line.push_str(SEPARATOR);
        push_level(&mut line, self.level, ansi);
        line.push_str(SEPARATOR);
        line.push('[');
        push_sanitized(&mut line, &self.module);
        line.push(']');
        if let Some(trace) = self.trace.as_deref().filter(|trace| !trace.is_empty()) {
            line.push_str(SEPARATOR);
            line.push('[');
            push_sanitized(&mut line, trace);
            line.push(']');
        }
        if let Some(scene) = scene_cell(&self.group, &self.user) {
            line.push_str(SEPARATOR);
            line.push_str(&scene);
        }
        line.push_str(SEPARATOR);
        push_sanitized(&mut line, &self.msg);
        for (key, value) in &self.fields {
            line.push(' ');
            push_sanitized(&mut line, key);
            line.push('=');
            push_sanitized(&mut line, value);
        }
        if let Some(duration) = self.duration_ms {
            line.push_str(SEPARATOR);
            let _ = write!(line, "{duration}ms");
            if self.is_slow(&options.slow) {
                line.push(' ');
                if ansi {
                    line.push_str(ANSI_REVERSE);
                    line.push_str(SLOW_MARK);
                    line.push_str(ANSI_RESET);
                } else {
                    line.push_str(SLOW_MARK);
                }
            }
        }
        line
    }
}

pub fn format_timestamp(time_ms: i64) -> String {
    Local
        .timestamp_millis_opt(time_ms)
        .single()
        .unwrap_or_default()
        .format(TIMESTAMP_FORMAT)
        .to_string()
}

pub fn level_cell(level: Level) -> &'static str {
    match level {
        Level::Debug => "DEBUG",
        Level::Info => "INFO ",
        Level::Warn => "WARN ",
        Level::Error => "ERROR",
    }
}

fn level_color(level: Level) -> &'static str {
    match level {
        Level::Debug => "\x1b[90m",
        Level::Info => "\x1b[32m",
        Level::Warn => "\x1b[33m",
        Level::Error => "\x1b[31m",
    }
}

fn push_level(line: &mut String, level: Level, ansi: bool) {
    let cell = level_cell(level);
    if ansi {
        line.push_str(level_color(level));
        line.push_str(cell);
        line.push_str(ANSI_RESET);
    } else {
        line.push_str(cell);
    }
}

pub fn scene_cell(group: &str, user: &str) -> Option<String> {
    match (group.is_empty(), user.is_empty()) {
        (true, true) => None,
        (false, true) => Some(format!("[group/{}]", sanitized(group))),
        (true, false) => Some(format!("[user/{}]", sanitized(user))),
        (false, false) => Some(format!(
            "[group/{} user/{}]",
            sanitized(group),
            sanitized(user)
        )),
    }
}

fn sanitized(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    push_sanitized(&mut out, text);
    out
}

fn push_sanitized(line: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '\n' => line.push_str("\\n"),
            '\r' => line.push_str("\\r"),
            '\t' => line.push('\t'),
            other => line.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use super::*;

    fn record(level: Level, module: &str, msg: &str) -> LogRecord {
        let mut record = LogRecord::new(level, module, msg, BTreeMap::new());
        record.time = FIXED_MS;
        record
    }

    const FIXED_MS: i64 = 1_771_081_445_123;

    fn stamp() -> String {
        format_timestamp(FIXED_MS)
    }

    fn expected(suffix: &str) -> String {
        format!("{}{}", stamp(), suffix)
    }

    fn plain() -> FormatOptions {
        FormatOptions::plain()
    }

    #[test]
    fn debug_line_is_char_exact() {
        let line = record(Level::Debug, "imagegen", "started").render(&plain());
        assert_eq!(line, expected(" | DEBUG | [imagegen] | started"));
        assert_eq!(line.matches(SEPARATOR).count(), 3, "固定四分段");
    }

    #[test]
    fn info_line_pads_level_column() {
        let line = record(Level::Info, "imagegen", "started").render(&plain());
        assert_eq!(line, expected(" | INFO  | [imagegen] | started"));
    }

    #[test]
    fn warn_line_keeps_padding_and_error_line_has_none() {
        let warn = record(Level::Warn, "imagegen", "slow upstream").render(&plain());
        assert_eq!(warn, expected(" | WARN  | [imagegen] | slow upstream"));
        let error = record(Level::Error, "imagegen", "failed").render(&plain());
        assert_eq!(error, expected(" | ERROR | [imagegen] | failed"));
    }

    #[test]
    fn level_column_is_always_five_wide() {
        assert_eq!(level_cell(Level::Debug).len(), LEVEL_WIDTH);
        assert_eq!(level_cell(Level::Info).len(), LEVEL_WIDTH);
        assert_eq!(level_cell(Level::Warn).len(), LEVEL_WIDTH);
        assert_eq!(level_cell(Level::Error).len(), LEVEL_WIDTH);
    }

    #[test]
    fn full_line_matches_documented_shape() {
        let mut record = record(
            Level::Info,
            "imagegen",
            "generated image url=https://x/y.png",
        );
        record.trace = Some("m3f9a1-17".to_string());
        record.group = "819E".to_string();
        record.user = "618F".to_string();
        record.duration_ms = Some(1284);
        let line =
            record.render(&FormatOptions::plain().with_slow(SlowThresholds::new(200, 1000, 1000)));
        assert_eq!(
            line,
            expected(
                " | INFO  | [imagegen] | [m3f9a1-17] | [group/819E user/618F] | generated image url=https://x/y.png | 1284ms [SLOW]"
            )
        );
    }

    #[test]
    fn absent_columns_do_not_add_separators() {
        let line = record(Level::Info, "gateway", "ping").render(&plain());
        assert_eq!(line, expected(" | INFO  | [gateway] | ping"));
        assert!(!line.contains("[]"), "缺省列不得输出空方括号");
        assert!(!line.ends_with(SEPARATOR), "缺省列不得留下尾部分隔符");
    }

    #[test]
    fn trace_only_line_has_one_extra_segment() {
        let mut record = record(Level::Warn, "gateway", "ping");
        record.trace = Some("t-7".to_string());
        assert_eq!(
            record.render(&plain()),
            expected(" | WARN  | [gateway] | [t-7] | ping")
        );
    }

    #[test]
    fn scene_column_degrades_to_the_present_half() {
        let mut only_group = record(Level::Info, "message", "x");
        only_group.group = "G1".to_string();
        assert_eq!(
            only_group.render(&plain()),
            expected(" | INFO  | [message] | [group/G1] | x")
        );
        let mut only_user = record(Level::Info, "message", "x");
        only_user.user = "U1".to_string();
        assert_eq!(
            only_user.render(&plain()),
            expected(" | INFO  | [message] | [user/U1] | x")
        );
    }

    #[test]
    fn empty_trace_is_treated_as_absent() {
        let mut record = record(Level::Info, "gateway", "x");
        record.trace = Some(String::new());
        assert_eq!(
            record.render(&plain()),
            expected(" | INFO  | [gateway] | x")
        );
    }

    #[test]
    fn fields_are_sorted_by_key() {
        let record = LogRecord::new(
            Level::Info,
            "gateway",
            "done",
            BTreeMap::from([
                ("zeta".to_string(), "1".to_string()),
                ("alpha".to_string(), "2".to_string()),
                ("mid".to_string(), "3".to_string()),
            ]),
        );
        let rendered = record.render(&plain());
        assert!(
            rendered.ends_with("done alpha=2 mid=3 zeta=1"),
            "{rendered}"
        );
    }

    #[test]
    fn duration_column_only_appears_with_duration() {
        let mut record = record(Level::Info, "db", "query");
        record.duration_ms = Some(12);
        assert_eq!(
            record.render(&plain()),
            expected(" | INFO  | [db] | query | 12ms")
        );
    }

    #[test]
    fn slow_mark_respects_strict_threshold() {
        let options = FormatOptions::plain();
        let mut at_limit = record(Level::Info, "gateway", "x");
        at_limit.duration_ms = Some(1500);
        assert!(
            !at_limit.render(&options).contains(SLOW_MARK),
            "等于阈值不算慢"
        );
        at_limit.duration_ms = Some(1501);
        assert!(
            at_limit.render(&options).contains(SLOW_MARK),
            "超过阈值需标记"
        );
        let mut db = record(Level::Info, "db", "x");
        db.duration_ms = Some(201);
        assert!(db.render(&options).contains(SLOW_MARK), "db 用 200ms 阈值");
        db.duration_ms = Some(200);
        assert!(!db.render(&options).contains(SLOW_MARK), "边界不算慢");
    }

    #[test]
    fn slow_mark_is_last() {
        let mut record = record(Level::Error, "db", "boom");
        record.duration_ms = Some(900);
        let line = record.render(&plain());
        assert!(line.ends_with("900ms [SLOW]"), "{line}");
    }

    #[test]
    fn ansi_rendering_colors_each_level() {
        let cases = [
            (Level::Debug, "\x1b[90mDEBUG\x1b[0m"),
            (Level::Info, "\x1b[32mINFO \x1b[0m"),
            (Level::Warn, "\x1b[33mWARN \x1b[0m"),
            (Level::Error, "\x1b[31mERROR\x1b[0m"),
        ];
        for (level, expected) in cases {
            let line = record(level, "gateway", "x").render_ansi(&FormatOptions::colored());
            assert!(line.contains(expected), "{level} 需带对应颜色: {line}");
        }
    }

    #[test]
    fn ansi_rendering_reverses_only_the_slow_mark() {
        let mut record = record(Level::Info, "db", "boom");
        record.duration_ms = Some(400);
        let line = record.render_ansi(&FormatOptions::colored());
        assert!(
            line.ends_with("400ms \x1b[7m[SLOW]\x1b[0m"),
            "只有 [SLOW] 需要反显: {line}"
        );
        assert_eq!(line.matches(ANSI_RESET).count(), 2, "级别与标记各一次复位");
    }

    #[test]
    fn plain_rendering_has_no_escape_codes() {
        let mut record = record(Level::Warn, "db", "boom");
        record.duration_ms = Some(400);
        let line = record.render(&FormatOptions::colored());
        assert!(!line.contains('\x1b'), "纯文本渲染不得含控制序列");
    }

    #[test]
    fn color_mode_resolution_is_stable() {
        assert!(ColorMode::Always.resolve());
        assert!(!ColorMode::Never.resolve());
        assert_eq!(FormatOptions::default().color, ColorMode::Auto);
        assert_eq!(FormatOptions::plain().color, ColorMode::Never);
        assert_eq!(FormatOptions::colored().color, ColorMode::Always);
    }

    #[test]
    fn newlines_are_escaped_in_a_single_line() {
        let mut record = record(Level::Info, "gateway", "line1\nline2");
        record.fields.insert("k".to_string(), "a\nb".to_string());
        let line = record.render(&plain());
        assert!(!line.contains('\n'), "渲染结果必须单行");
        assert!(line.ends_with("line1\\nline2 k=a\\nb"), "{line}");
    }

    #[test]
    fn duration_helpers_round_trip() {
        let record = LogRecord::new(Level::Info, "db", "x", BTreeMap::new())
            .with_duration(Duration::from_millis(250));
        assert_eq!(record.duration_ms, Some(250));
        let zero = LogRecord::new(Level::Info, "db", "x", BTreeMap::new());
        assert_eq!(zero.duration_ms, None);
    }

    #[test]
    fn timestamp_is_millisecond_precision() {
        let rendered = format_timestamp(FIXED_MS);
        let local = Local
            .timestamp_millis_opt(FIXED_MS)
            .single()
            .expect("合法毫秒时间戳")
            .format(TIMESTAMP_FORMAT)
            .to_string();
        assert_eq!(rendered, local, "时间戳需按本地时区渲染");
        assert_eq!(rendered.len(), 23, "YYYY-MM-DD HH:MM:SS.mmm 定长");
        let bytes = rendered.as_bytes();
        assert_eq!(bytes[4], b'-');
        assert_eq!(bytes[7], b'-');
        assert_eq!(bytes[10], b' ');
        assert_eq!(bytes[13], b':');
        assert_eq!(bytes[16], b':');
        assert_eq!(bytes[19], b'.');
        assert!(
            rendered
                .as_bytes()
                .iter()
                .enumerate()
                .all(|(index, byte)| matches!(index, 4 | 7 | 10 | 13 | 16 | 19)
                    || byte.is_ascii_digit()),
            "除分隔符外只能是数字: {rendered}"
        );
    }

    #[test]
    fn timestamp_renders_in_the_local_zone() {
        let midnight = Local
            .with_ymd_and_hms(2026, 2, 14, 0, 0, 0)
            .single()
            .expect("合法本地时间");
        let rendered = format_timestamp(midnight.timestamp_millis());
        assert_eq!(rendered, "2026-02-14 00:00:00.000");
        let before = format_timestamp(midnight.timestamp_millis() - 1);
        assert_eq!(before, "2026-02-13 23:59:59.999");
    }

    #[test]
    fn duration_of_zero_still_renders_a_column() {
        let mut record = record(Level::Info, "db", "x");
        record.duration_ms = Some(0);
        assert!(record.render(&plain()).ends_with("| 0ms"));
    }
}
