use std::collections::BTreeMap;
use std::sync::RwLock;
use std::time::Duration;

use tokio::time::Instant;

use crate::error::PluginError;

pub const DEFAULT_BAN_MESSAGE: &str = "您的账号因违反Bot使用协议已被封禁";
pub const BAN_MESSAGE_LIMIT: usize = 2000;
pub const BAN_THROTTLE: Duration = Duration::from_secs(15);
const BAN_PRUNE_AT: usize = 4096;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BanEntry {
    pub openid: String,
    pub message: String,
    pub note: String,
    pub at: String,
}

impl BanEntry {
    pub fn new(openid: impl Into<String>) -> Self {
        Self {
            openid: openid.into(),
            message: String::new(),
            note: String::new(),
            at: String::new(),
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    pub fn with_at(mut self, at: impl Into<String>) -> Self {
        self.at = at.into();
        self
    }

    pub fn text(&self) -> String {
        if self.message.trim().is_empty() {
            return DEFAULT_BAN_MESSAGE.to_string();
        }
        self.message.clone()
    }
}

#[derive(Debug, Default)]
pub struct BanList {
    entries: RwLock<BTreeMap<String, BanEntry>>,
    last: RwLock<BTreeMap<String, Instant>>,
}

impl BanList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_entries(entries: impl IntoIterator<Item = BanEntry>) -> Self {
        let list = Self::new();
        list.load(entries);
        list
    }

    pub fn load(&self, entries: impl IntoIterator<Item = BanEntry>) {
        let mut next = BTreeMap::new();
        for entry in entries {
            let openid = entry.openid.trim().to_string();
            if openid.is_empty() {
                continue;
            }
            next.insert(openid, entry);
        }
        let mut guard = self.entries.write().unwrap_or_else(|p| p.into_inner());
        *guard = next;
    }

    pub fn put(&self, entry: BanEntry) -> Result<(), PluginError> {
        let openid = entry.openid.trim().to_string();
        if openid.is_empty() {
            return Err(PluginError::Invalid("openid 不能为空".to_string()));
        }
        if openid.contains(char::is_whitespace) {
            return Err(PluginError::Invalid("openid 不能包含空白字符".to_string()));
        }
        let length = entry.message.chars().count();
        if length > BAN_MESSAGE_LIMIT {
            return Err(PluginError::Invalid(format!(
                "文案 {length} 字, 超过上限 {BAN_MESSAGE_LIMIT} 字"
            )));
        }
        let mut guard = self.entries.write().unwrap_or_else(|p| p.into_inner());
        let mut stored = entry;
        stored.openid = openid.clone();
        if stored.at.is_empty() {
            if let Some(previous) = guard.get(&openid) {
                stored.at = previous.at.clone();
            }
        }
        guard.insert(openid, stored);
        Ok(())
    }

    pub fn delete(&self, openid: &str) -> Result<(), PluginError> {
        let openid = openid.trim();
        let mut guard = self.entries.write().unwrap_or_else(|p| p.into_inner());
        if guard.remove(openid).is_none() {
            return Err(PluginError::Invalid("该用户不在封禁名单里".to_string()));
        }
        drop(guard);
        let mut last = self.last.write().unwrap_or_else(|p| p.into_inner());
        last.remove(openid);
        Ok(())
    }

    pub fn list(&self) -> Vec<BanEntry> {
        let guard = self.entries.read().unwrap_or_else(|p| p.into_inner());
        let mut list: Vec<BanEntry> = guard.values().cloned().collect();
        list.sort_by(|left, right| {
            right
                .at
                .cmp(&left.at)
                .then_with(|| left.openid.cmp(&right.openid))
        });
        list
    }

    pub fn contains(&self, openid: &str) -> bool {
        let guard = self.entries.read().unwrap_or_else(|p| p.into_inner());
        guard.contains_key(openid)
    }

    pub fn len(&self) -> usize {
        let guard = self.entries.read().unwrap_or_else(|p| p.into_inner());
        guard.len()
    }

    pub fn is_empty(&self) -> bool {
        let guard = self.entries.read().unwrap_or_else(|p| p.into_inner());
        guard.is_empty()
    }

    pub fn hit(&self, openid: &str) -> Option<String> {
        self.hit_at(openid, Instant::now())
    }

    pub fn hit_at(&self, openid: &str, now: Instant) -> Option<String> {
        if openid.is_empty() {
            return None;
        }
        let text = {
            let guard = self.entries.read().unwrap_or_else(|p| p.into_inner());
            guard.get(openid).map(BanEntry::text)?
        };
        let mut last = self.last.write().unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = last.get(openid) {
            if now.saturating_duration_since(*previous) < BAN_THROTTLE {
                return None;
            }
        }
        if last.len() > BAN_PRUNE_AT {
            last.retain(|_, seen| now.saturating_duration_since(*seen) >= BAN_THROTTLE);
        }
        last.insert(openid.to_string(), now);
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_text_falls_back_to_default() {
        let entry = BanEntry::new("u1");
        assert_eq!(entry.text(), DEFAULT_BAN_MESSAGE);
        let entry = BanEntry::new("u1").with_message("   ");
        assert_eq!(entry.text(), DEFAULT_BAN_MESSAGE);
        let entry = BanEntry::new("u1").with_message("违反群规");
        assert_eq!(entry.text(), "违反群规");
    }

    #[test]
    fn hit_reports_once_per_throttle() {
        let list = BanList::new();
        list.put(BanEntry::new("u1").with_message("禁言中"))
            .unwrap();
        let now = Instant::now();
        assert_eq!(list.hit_at("u1", now).as_deref(), Some("禁言中"));
        assert_eq!(list.hit_at("u1", now + Duration::from_secs(5)), None);
        assert_eq!(
            list.hit_at("u1", now + BAN_THROTTLE).as_deref(),
            Some("禁言中"),
            "节流窗口过后应当再回一次"
        );
    }

    #[test]
    fn unknown_or_empty_user_never_hits() {
        let list = BanList::new();
        assert!(list.hit("").is_none());
        assert!(list.hit("u2").is_none());
    }

    #[test]
    fn put_validates_openid_and_message() {
        let list = BanList::new();
        assert_eq!(
            list.put(BanEntry::new("  ")).unwrap_err().to_string(),
            "参数不合法: openid 不能为空"
        );
        assert_eq!(
            list.put(BanEntry::new("u 1")).unwrap_err().to_string(),
            "参数不合法: openid 不能包含空白字符"
        );
        let long = "字".repeat(BAN_MESSAGE_LIMIT + 1);
        assert_eq!(
            list.put(BanEntry::new("u1").with_message(long))
                .unwrap_err()
                .to_string(),
            "参数不合法: 文案 2001 字, 超过上限 2000 字"
        );
    }

    #[test]
    fn delete_reports_missing_entry() {
        let list = BanList::new();
        assert_eq!(
            list.delete("u1").unwrap_err().to_string(),
            "参数不合法: 该用户不在封禁名单里"
        );
        list.put(BanEntry::new("u1")).unwrap();
        assert!(list.contains("u1"));
        list.delete("u1").unwrap();
        assert!(!list.contains("u1"));
    }

    #[test]
    fn list_sorts_by_time_desc() {
        let list = BanList::from_entries([
            BanEntry::new("b").with_at("2024-01-01T00:00:00Z"),
            BanEntry::new("a").with_at("2024-02-01T00:00:00Z"),
            BanEntry::new("c").with_at("2024-01-01T00:00:00Z"),
        ]);
        let openids: Vec<String> = list.list().into_iter().map(|e| e.openid).collect();
        assert_eq!(
            openids,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(list.len(), 3);
        assert!(!list.is_empty());
    }

    #[test]
    fn throttle_table_is_pruned_past_the_limit() {
        let list = BanList::new();
        let start = Instant::now();
        for index in 0..=BAN_PRUNE_AT {
            let openid = format!("u{index}");
            list.put(BanEntry::new(openid.clone()))
                .expect("入名单要成功");
            assert!(list.hit_at(&openid, start).is_some(), "首次命中要回话");
        }
        let later = start + BAN_THROTTLE;
        assert!(
            list.hit_at("u0", later).is_some(),
            "节流表超限后过期记录被清掉, 应当再回一次"
        );
        assert_eq!(list.len(), BAN_PRUNE_AT + 1);
    }

    #[test]
    fn put_keeps_original_ban_time() {
        let list = BanList::new();
        list.put(BanEntry::new("u1").with_at("2024-01-01T00:00:00Z"))
            .unwrap();
        list.put(BanEntry::new("u1").with_message("再犯")).unwrap();
        let entry = list.list().into_iter().next().unwrap();
        assert_eq!(entry.at, "2024-01-01T00:00:00Z");
        assert_eq!(entry.message, "再犯");
    }
}
