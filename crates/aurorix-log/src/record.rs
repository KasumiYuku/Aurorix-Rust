use std::collections::BTreeMap;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::level::Level;

pub const MAX_MESSAGE_BYTES: usize = 8 << 10;

pub const TRUNCATION_MARK: &str = "…(截断)";

pub const RECORD_OVERHEAD_BYTES: usize = 64;

pub const DURATION_BYTES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogRecord {
    pub id: u64,
    pub time: i64,
    pub level: Level,
    #[serde(rename = "scope")]
    pub module: String,
    pub msg: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl LogRecord {
    pub fn new(
        level: Level,
        module: impl Into<String>,
        msg: impl Into<String>,
        fields: BTreeMap<String, String>,
    ) -> Self {
        Self {
            id: 0,
            time: Utc::now().timestamp_millis(),
            level,
            module: module.into(),
            msg: msg.into(),
            group: String::new(),
            user: String::new(),
            trace: None,
            fields,
            duration_ms: None,
        }
    }

    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    pub fn with_trace(mut self, trace: impl Into<String>) -> Self {
        let trace = trace.into();
        if !trace.is_empty() {
            self.trace = Some(trace);
        }
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration_ms = Some(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
        self
    }

    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    pub fn size(&self) -> usize {
        let fields: usize = self
            .fields
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum();
        self.msg.len()
            + self.module.len()
            + self.group.len()
            + self.user.len()
            + self.trace.as_deref().unwrap_or("").len()
            + self.level.label().len()
            + fields
            + self.duration_ms.map(|_| DURATION_BYTES).unwrap_or(0)
            + RECORD_OVERHEAD_BYTES
    }

    pub fn fitted(mut self) -> Self {
        self.msg = truncate_message(&self.msg);
        self
    }
}

pub fn truncate_message(msg: &str) -> String {
    if msg.len() <= MAX_MESSAGE_BYTES {
        return msg.to_string();
    }
    let mut cut = MAX_MESSAGE_BYTES;
    while cut > 0 && !msg.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + TRUNCATION_MARK.len());
    out.push_str(&msg[..cut]);
    out.push_str(TRUNCATION_MARK);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_message_is_untouched() {
        assert_eq!(truncate_message("你好"), "你好");
    }

    #[test]
    fn message_at_exact_limit_is_untouched() {
        let source = "x".repeat(MAX_MESSAGE_BYTES);
        assert_eq!(truncate_message(&source), source, "恰好 8 KiB 不得截断");
        let longer = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert!(
            truncate_message(&longer).ends_with(TRUNCATION_MARK),
            "超一字节需截断"
        );
    }

    #[test]
    fn long_message_is_cut_at_utf8_boundary() {
        let source = "中".repeat(MAX_MESSAGE_BYTES);
        let clipped = truncate_message(&source);
        assert!(clipped.ends_with(TRUNCATION_MARK), "截断后需带标记");
        let body = clipped.trim_end_matches(TRUNCATION_MARK);
        assert!(body.len() <= MAX_MESSAGE_BYTES, "截断后正文需不超过 8 KiB");
        assert!(body.len() + 3 > MAX_MESSAGE_BYTES, "截断需贴近上限");
        assert_eq!(body.chars().count() * 3, body.len(), "需在字符边界截断");
    }

    #[test]
    fn size_counts_message_and_fields() {
        let record = LogRecord::new(
            Level::Info,
            "gateway",
            "abc",
            BTreeMap::from([("k".to_string(), "vv".to_string())]),
        );
        assert_eq!(record.size(), 3 + 7 + 4 + 1 + 2 + RECORD_OVERHEAD_BYTES);
    }

    #[test]
    fn size_counts_duration_only_when_present() {
        let record = LogRecord::new(Level::Info, "db", "q", BTreeMap::new()).with_duration_ms(12);
        assert_eq!(
            record.size(),
            1 + 2 + 4 + DURATION_BYTES + RECORD_OVERHEAD_BYTES
        );
    }

    #[test]
    fn trace_ignores_empty_values() {
        let record = LogRecord::new(Level::Info, "m", "x", BTreeMap::new()).with_trace("");
        assert_eq!(record.trace, None);
        let record = record.with_trace("t-1");
        assert_eq!(record.trace.as_deref(), Some("t-1"));
    }

    #[test]
    fn duration_is_serialized_only_when_present() {
        let plain = LogRecord::new(Level::Info, "db", "q", BTreeMap::new());
        let encoded = serde_json::to_value(&plain).expect("序列化");
        assert!(encoded.get("duration_ms").is_none(), "无耗时不出现该键");
        let timed = plain.with_duration(Duration::from_millis(7));
        let encoded = serde_json::to_value(&timed).expect("序列化");
        assert_eq!(encoded["duration_ms"], serde_json::json!(7));
    }
}
