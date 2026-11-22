use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use dashmap::DashMap;

pub const DEFAULT_DEDUP_TTL: Duration = Duration::from_secs(300);

pub const DEFAULT_DEDUP_CAPACITY: usize = 8192;

pub struct DedupWindow {
    ttl: Duration,
    capacity: usize,
    seen: DashMap<String, Instant>,
    order: Mutex<VecDeque<String>>,
}

impl DedupWindow {
    pub fn new(ttl: Duration) -> Self {
        Self::with_capacity(ttl, DEFAULT_DEDUP_CAPACITY)
    }

    pub fn with_capacity(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity: capacity.max(1),
            seen: DashMap::new(),
            order: Mutex::new(VecDeque::new()),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn remember(&self, key: &str) {
        let mut order = self
            .order
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        order.push_back(key.to_string());
        while order.len() > self.capacity * 2 {
            order.pop_front();
        }
        while self.seen.len() > self.capacity {
            let Some(oldest) = order.pop_front() else {
                break;
            };
            self.seen.remove(&oldest);
        }
    }

    pub fn with_default_ttl() -> Self {
        Self::new(DEFAULT_DEDUP_TTL)
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn check_and_mark(&self, event_id: &str) -> bool {
        if event_id.is_empty() {
            return true;
        }
        let now = Instant::now();
        if let Some(entry) = self.seen.get(event_id) {
            if now.duration_since(*entry) < self.ttl {
                return false;
            }
        }
        self.seen.insert(event_id.to_string(), now);
        self.remember(event_id);
        true
    }

    pub fn check_and_mark_event(&self, event_id: &str, message_id: &str) -> bool {
        if !event_id.is_empty() {
            return self.check_and_mark(event_id);
        }
        self.check_and_mark(message_id)
    }

    pub fn seen(&self, event_id: &str) -> bool {
        self.seen.contains_key(event_id)
    }

    pub fn forget(&self, event_id: &str) {
        self.seen.remove(event_id);
    }

    pub fn purge(&self) -> usize {
        let now = Instant::now();
        let expired: Vec<String> = self
            .seen
            .iter()
            .filter(|entry| now.duration_since(*entry.value()) >= self.ttl)
            .map(|entry| entry.key().clone())
            .collect();
        let count = expired.len();
        for key in expired {
            self.seen.remove(&key);
        }
        count
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

impl Default for DedupWindow {
    fn default() -> Self {
        Self::with_default_ttl()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ttl_is_five_minutes() {
        assert_eq!(DEFAULT_DEDUP_TTL, Duration::from_secs(300));
        assert_eq!(DedupWindow::default().ttl(), DEFAULT_DEDUP_TTL);
    }

    #[test]
    fn second_seen_event_is_duplicate() {
        let window = DedupWindow::new(Duration::from_secs(60));
        assert!(window.check_and_mark("e1"), "首次要放行");
        assert!(!window.check_and_mark("e1"), "第二次要判重");
        assert!(window.check_and_mark("e2"), "不同事件要放行");
        assert!(window.seen("e1"));
        assert_eq!(window.len(), 2);
        assert!(!window.is_empty());
        window.forget("e1");
        assert!(window.check_and_mark("e1"), "忘记之后要重新放行");
    }

    #[test]
    fn ttl_expiry_allows_again() {
        let window = DedupWindow::new(Duration::from_millis(40));
        assert!(window.check_and_mark("e1"));
        assert!(!window.check_and_mark("e1"));
        std::thread::sleep(Duration::from_millis(60));
        assert!(window.check_and_mark("e1"), "TTL 过后要允许");
        assert!(!window.check_and_mark("e1"));
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(window.purge(), 1, "过期条目要被清掉");
        assert!(window.is_empty());
    }

    #[test]
    fn missing_ids_are_never_blocked() {
        let window = DedupWindow::new(Duration::from_secs(60));
        assert!(window.check_and_mark(""));
        assert!(window.check_and_mark(""));
        assert_eq!(window.len(), 0, "空 id 不入表");
    }

    #[test]
    fn event_id_preferred_over_message_id() {
        let window = DedupWindow::new(Duration::from_secs(60));
        assert!(window.check_and_mark_event("e1", "m1"));
        assert!(
            !window.check_and_mark_event("e1", "m2"),
            "event_id 相同就是重复"
        );
        assert!(window.check_and_mark_event("", "m1"));
        assert!(
            !window.check_and_mark_event("", "m1"),
            "没有 event_id 时用 message_id"
        );
    }
}
