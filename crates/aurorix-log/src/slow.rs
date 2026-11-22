use serde::{Deserialize, Serialize};

use crate::record::LogRecord;

pub const DEFAULT_DB_MS: u64 = 200;

pub const DEFAULT_RENDER_MS: u64 = 1000;

pub const DEFAULT_UPSTREAM_MS: u64 = 1500;

pub const DB_MODULE_MARKER: &str = "db";

pub const RENDER_MODULE_MARKER: &str = "render";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlowThresholds {
    pub db_ms: u64,
    pub render_ms: u64,
    pub upstream_ms: u64,
}

impl Default for SlowThresholds {
    fn default() -> Self {
        Self {
            db_ms: DEFAULT_DB_MS,
            render_ms: DEFAULT_RENDER_MS,
            upstream_ms: DEFAULT_UPSTREAM_MS,
        }
    }
}

impl SlowThresholds {
    pub fn new(db_ms: u64, render_ms: u64, upstream_ms: u64) -> Self {
        Self {
            db_ms,
            render_ms,
            upstream_ms,
        }
    }

    pub fn with_db_ms(mut self, db_ms: u64) -> Self {
        self.db_ms = db_ms;
        self
    }

    pub fn with_render_ms(mut self, render_ms: u64) -> Self {
        self.render_ms = render_ms;
        self
    }

    pub fn with_upstream_ms(mut self, upstream_ms: u64) -> Self {
        self.upstream_ms = upstream_ms;
        self
    }

    pub fn threshold_ms(&self, module: &str) -> u64 {
        let lowered = module.to_lowercase();
        if lowered.contains(DB_MODULE_MARKER) {
            self.db_ms
        } else if lowered.contains(RENDER_MODULE_MARKER) {
            self.render_ms
        } else {
            self.upstream_ms
        }
    }

    pub fn is_slow(&self, module: &str, duration_ms: u64) -> bool {
        duration_ms > self.threshold_ms(module)
    }
}

impl LogRecord {
    pub fn threshold_ms(&self, thresholds: &SlowThresholds) -> u64 {
        thresholds.threshold_ms(&self.module)
    }

    pub fn is_slow(&self, thresholds: &SlowThresholds) -> bool {
        match self.duration_ms {
            Some(duration) => thresholds.is_slow(&self.module, duration),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use super::*;
    use crate::level::Level;

    #[test]
    fn defaults_match_documented_thresholds() {
        let thresholds = SlowThresholds::default();
        assert_eq!(thresholds.db_ms, 200, "db 阈值默认 200ms");
        assert_eq!(thresholds.render_ms, 1000, "渲染阈值默认 1000ms");
        assert_eq!(thresholds.upstream_ms, 1500, "上游阈值默认 1500ms");
    }

    #[test]
    fn module_prefix_selects_the_threshold() {
        let thresholds = SlowThresholds::default();
        assert_eq!(thresholds.threshold_ms("db"), 200);
        assert_eq!(thresholds.threshold_ms("storage-db"), 200);
        assert_eq!(thresholds.threshold_ms("render-kit"), 1000);
        assert_eq!(thresholds.threshold_ms("gateway"), 1500);
    }

    #[test]
    fn slow_boundary_is_strictly_greater() {
        let thresholds = SlowThresholds::default();
        assert!(!thresholds.is_slow("gateway", 1500), "等于阈值不算慢");
        assert!(thresholds.is_slow("gateway", 1501), "超过阈值才算慢");
        assert!(!thresholds.is_slow("db", 200), "等于 db 阈值不算慢");
        assert!(thresholds.is_slow("db", 201), "超过 db 阈值才算慢");
    }

    #[test]
    fn record_without_duration_is_never_slow() {
        let record = LogRecord::new(Level::Info, "db", "x", BTreeMap::new());
        assert!(!record.is_slow(&SlowThresholds::default()), "无耗时不算慢");
    }

    #[test]
    fn record_uses_module_specific_threshold() {
        let thresholds = SlowThresholds::default();
        let db = LogRecord::new(Level::Info, "db", "x", BTreeMap::new())
            .with_duration(Duration::from_millis(400));
        assert!(db.is_slow(&thresholds), "db 400ms 需判慢");
        let render = LogRecord::new(Level::Info, "render", "x", BTreeMap::new())
            .with_duration(Duration::from_millis(400));
        assert!(!render.is_slow(&thresholds), "render 400ms 未达 1000ms");
    }
}
