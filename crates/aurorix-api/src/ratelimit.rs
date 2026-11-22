use std::collections::VecDeque;
use std::time::{Duration, Instant};

use dashmap::DashMap;

use crate::error::ApiError;

pub const PUSH_KEY: &str = "push";
pub const PUSH_RATE_LIMIT: u32 = 20;
pub const PUSH_RATE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    limit: u32,
    window: Duration,
}

pub struct RateLimiter {
    rules: DashMap<String, Window>,
    hits: DashMap<String, VecDeque<Instant>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        let limiter = Self {
            rules: DashMap::new(),
            hits: DashMap::new(),
        };
        limiter.configure(PUSH_KEY, PUSH_RATE_LIMIT, PUSH_RATE_WINDOW);
        limiter
    }

    pub fn without_rules() -> Self {
        Self {
            rules: DashMap::new(),
            hits: DashMap::new(),
        }
    }

    pub fn configure(&self, key: impl Into<String>, limit: u32, window: Duration) {
        self.rules.insert(key.into(), Window { limit, window });
    }

    pub fn rule(&self, key: &str) -> Option<(u32, Duration)> {
        self.rules.get(key).map(|rule| (rule.limit, rule.window))
    }

    pub fn check(&self, key: &str) -> Result<(), ApiError> {
        let Some(rule) = self.rules.get(key).map(|entry| *entry) else {
            return Ok(());
        };
        let now = Instant::now();
        let mut hits = self.hits.entry(key.to_string()).or_default();
        while let Some(front) = hits.front() {
            if now.duration_since(*front) < rule.window {
                break;
            }
            hits.pop_front();
        }
        if hits.len() >= rule.limit as usize {
            return Err(ApiError::RateLimited {
                key: key.to_string(),
                limit: rule.limit,
                window_ms: rule.window.as_millis() as u64,
            });
        }
        hits.push_back(now);
        Ok(())
    }

    pub fn remaining(&self, key: &str) -> u32 {
        let Some(rule) = self.rules.get(key).map(|entry| *entry) else {
            return u32::MAX;
        };
        let now = Instant::now();
        let used = match self.hits.get(key) {
            Some(hits) => hits
                .iter()
                .filter(|at| now.duration_since(**at) < rule.window)
                .count() as u32,
            None => 0,
        };
        rule.limit.saturating_sub(used)
    }

    pub fn reset(&self, key: &str) {
        self.hits.remove(key);
    }

    pub fn hits(&self, key: &str) -> usize {
        self.hits.get(key).map(|hits| hits.len()).unwrap_or(0)
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_defaults_match_go() {
        let limiter = RateLimiter::new();
        assert_eq!(PUSH_RATE_LIMIT, 20);
        assert_eq!(PUSH_RATE_WINDOW, Duration::from_secs(60));
        assert_eq!(
            limiter.rule(PUSH_KEY),
            Some((PUSH_RATE_LIMIT, PUSH_RATE_WINDOW))
        );
        assert_eq!(limiter.rule("unknown"), None);
        assert_eq!(limiter.remaining(PUSH_KEY), 20);
    }

    #[test]
    fn unknown_key_is_unlimited() {
        let limiter = RateLimiter::new();
        for _ in 0..100 {
            assert!(limiter.check("other").is_ok(), "没有规则就不该限流");
        }
        assert_eq!(limiter.remaining("other"), u32::MAX);
    }

    #[test]
    fn window_slides_and_reopens() {
        let limiter = RateLimiter::without_rules();
        limiter.configure("t", 2, Duration::from_millis(60));
        assert!(limiter.check("t").is_ok());
        assert!(limiter.check("t").is_ok());
        assert_eq!(limiter.remaining("t"), 0);
        let err = limiter.check("t").unwrap_err();
        match &err {
            ApiError::RateLimited {
                key,
                limit,
                window_ms,
            } => {
                assert_eq!(key, "t");
                assert_eq!(*limit, 2);
                assert_eq!(*window_ms, 60);
            }
            other => panic!("限流错误类型不对: {other}"),
        }
        std::thread::sleep(Duration::from_millis(90));
        assert!(limiter.check("t").is_ok(), "窗口过后要重新放行");
        limiter.reset("t");
        assert_eq!(limiter.hits("t"), 0);
    }

    #[test]
    fn push_limit_counts_twenty_per_minute() {
        let limiter = RateLimiter::new();
        for index in 0..PUSH_RATE_LIMIT {
            assert!(limiter.check(PUSH_KEY).is_ok(), "第 {index} 次不该被限");
        }
        assert!(limiter.check(PUSH_KEY).is_err(), "第 21 次必须被限");
        assert_eq!(limiter.remaining(PUSH_KEY), 0);
        assert_eq!(limiter.hits(PUSH_KEY), 20);
    }

    #[test]
    fn sliding_window_keeps_only_recent_hits() {
        let limiter = RateLimiter::without_rules();
        limiter.configure("s", 5, Duration::from_millis(40));
        for _ in 0..3 {
            limiter.check("s").unwrap();
        }
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(limiter.hits("s"), 3, "过期命中还没被清");
        limiter.check("s").unwrap();
        assert_eq!(limiter.hits("s"), 1, "窗口外的命中要被挤出去");
        assert_eq!(limiter.remaining("s"), 4);
    }
}
