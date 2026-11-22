use std::collections::{BTreeMap, BTreeSet, VecDeque};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::level::Level;
use crate::record::LogRecord;

pub const DEFAULT_RING_CAPACITY: usize = 2048;

pub const DEFAULT_RING_BYTES: usize = 8 << 20;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingStats {
    pub entries: usize,
    pub bytes: usize,
    pub dropped: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PushOutcome {
    pub written: usize,
    pub evicted: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingSummary {
    pub levels: BTreeMap<String, u64>,
    pub scopes: BTreeMap<String, u64>,
}

#[derive(Debug, Default)]
struct Inner {
    buf: VecDeque<LogRecord>,
    bytes: usize,
    dropped: u64,
    total: u64,
}

#[derive(Debug)]
pub struct Ring {
    capacity: usize,
    byte_budget: usize,
    inner: Mutex<Inner>,
}

impl Default for Ring {
    fn default() -> Self {
        Self::new(DEFAULT_RING_CAPACITY, DEFAULT_RING_BYTES)
    }
}

impl Ring {
    pub fn new(capacity: usize, byte_budget: usize) -> Self {
        Self {
            capacity: if capacity == 0 {
                DEFAULT_RING_CAPACITY
            } else {
                capacity
            },
            byte_budget: if byte_budget == 0 {
                DEFAULT_RING_BYTES
            } else {
                byte_budget
            },
            inner: Mutex::new(Inner::default()),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn byte_budget(&self) -> usize {
        self.byte_budget
    }

    pub fn push(&self, record: LogRecord) -> PushOutcome {
        let size = record.size();
        let mut inner = self.inner.lock();
        inner.buf.push_back(record);
        inner.bytes += size;
        inner.total += 1;
        let mut evicted = 0usize;
        while inner.buf.len() > 1
            && (inner.buf.len() > self.capacity || inner.bytes > self.byte_budget)
        {
            if let Some(oldest) = inner.buf.pop_front() {
                inner.bytes -= oldest.size();
                evicted += 1;
            }
        }
        inner.dropped += evicted as u64;
        PushOutcome {
            written: 1,
            evicted,
        }
    }

    pub fn len(&self) -> usize {
        self.inner.lock().buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter_newest_first(&self, limit: usize) -> Vec<LogRecord> {
        let inner = self.inner.lock();
        let stored = inner.buf.len();
        let take = if limit == 0 || limit > stored {
            stored
        } else {
            limit
        };
        inner.buf.iter().rev().take(take).cloned().collect()
    }

    pub fn stats(&self) -> RingStats {
        let inner = self.inner.lock();
        RingStats {
            entries: inner.buf.len(),
            bytes: inner.bytes,
            dropped: inner.dropped,
            total: inner.total,
        }
    }

    pub fn summary(&self) -> RingSummary {
        let inner = self.inner.lock();
        let mut summary = RingSummary::default();
        for record in &inner.buf {
            *summary
                .levels
                .entry(record.level.as_str().to_string())
                .or_insert(0) += 1;
            *summary.scopes.entry(record.module.clone()).or_insert(0) += 1;
        }
        summary
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.buf.clear();
        inner.bytes = 0;
    }
}

pub const DEFAULT_QUERY_LIMIT: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RingFilter {
    pub min_level: Option<Level>,
    pub module: Option<String>,
    pub trace: Option<String>,
    pub group: Option<String>,
    pub user: Option<String>,
    pub keyword: Option<String>,
    pub limit: usize,
}

impl RingFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_min_level(mut self, min_level: Level) -> Self {
        self.min_level = Some(min_level);
        self
    }

    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module = Some(module.into());
        self
    }

    pub fn with_trace(mut self, trace: impl Into<String>) -> Self {
        self.trace = Some(trace.into());
        self
    }

    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    pub fn with_keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keyword = Some(keyword.into());
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn effective_limit(&self) -> usize {
        if self.limit == 0 {
            DEFAULT_QUERY_LIMIT
        } else {
            self.limit
        }
    }

    pub fn matches(&self, record: &LogRecord) -> bool {
        if let Some(min_level) = self.min_level {
            if record.level.rank() < min_level.rank() {
                return false;
            }
        }
        if let Some(module) = filter_text(&self.module) {
            if record.module != module {
                return false;
            }
        }
        if let Some(trace) = filter_text(&self.trace) {
            if record.trace.as_deref() != Some(trace) {
                return false;
            }
        }
        if let Some(group) = filter_text(&self.group) {
            if record.group != group {
                return false;
            }
        }
        if let Some(user) = filter_text(&self.user) {
            if record.user != user {
                return false;
            }
        }
        if let Some(keyword) = filter_text(&self.keyword) {
            if !matches_keyword(record, keyword) {
                return false;
            }
        }
        true
    }
}

fn filter_text(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn matches_keyword(record: &LogRecord, keyword: &str) -> bool {
    let needle = keyword.to_lowercase();
    if record.msg.to_lowercase().contains(&needle) {
        return true;
    }
    if record.module.to_lowercase().contains(&needle) {
        return true;
    }
    record
        .fields
        .values()
        .any(|value| value.to_lowercase().contains(&needle))
}

impl Ring {
    pub fn query(&self, filter: &RingFilter) -> Vec<LogRecord> {
        let limit = filter.effective_limit();
        let inner = self.inner.lock();
        inner
            .buf
            .iter()
            .rev()
            .filter(|record| filter.matches(record))
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn scopes(&self) -> Vec<String> {
        let inner = self.inner.lock();
        let mut names: BTreeSet<&str> = BTreeSet::new();
        for record in &inner.buf {
            names.insert(record.module.as_str());
        }
        names.into_iter().map(str::to_string).collect()
    }
}
