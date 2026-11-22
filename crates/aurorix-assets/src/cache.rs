use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::{Mutex, RwLock};

pub const DEFAULT_CAPACITY: usize = 512;

pub const DEFAULT_URL_CACHE_SECONDS: u64 = 21600;

pub const DEFAULT_URL_CACHE_RENEWALS: u32 = 6;

pub const DEFAULT_URL_CACHE_PROBE_PERCENT: u8 = 25;

pub const MIN_URL_CACHE_PROBE_PERCENT: u8 = 1;

pub const MAX_URL_CACHE_PROBE_PERCENT: u8 = 100;

pub const DEFAULT_URL_CACHE_POLICY: UrlCachePolicy = UrlCachePolicy {
    seconds: DEFAULT_URL_CACHE_SECONDS,
    verify: true,
    renewals: DEFAULT_URL_CACHE_RENEWALS,
    probe_percent: DEFAULT_URL_CACHE_PROBE_PERCENT,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DedupEntry {
    pub url: String,
    pub provider: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UrlCachePolicy {
    pub seconds: u64,
    pub verify: bool,
    pub renewals: u32,
    pub probe_percent: u8,
}

impl Default for UrlCachePolicy {
    fn default() -> Self {
        DEFAULT_URL_CACHE_POLICY
    }
}

impl UrlCachePolicy {
    pub fn new(seconds: u64, verify: bool, renewals: u32, probe_percent: u8) -> Self {
        Self {
            seconds,
            verify,
            renewals,
            probe_percent,
        }
        .sanitized()
    }

    pub fn sanitized(self) -> Self {
        Self {
            seconds: self.seconds,
            verify: self.verify,
            renewals: self.renewals,
            probe_percent: self
                .probe_percent
                .clamp(MIN_URL_CACHE_PROBE_PERCENT, MAX_URL_CACHE_PROBE_PERCENT),
        }
    }

    pub fn lifetime_ms(&self) -> u64 {
        self.seconds.saturating_mul(1000)
    }

    pub fn probe_interval_ms(&self) -> u64 {
        self.lifetime_ms() / 100 * u64::from(self.probe_percent)
    }
}

fn policy_cell() -> &'static RwLock<UrlCachePolicy> {
    static POLICY: OnceLock<RwLock<UrlCachePolicy>> = OnceLock::new();
    POLICY.get_or_init(|| RwLock::new(DEFAULT_URL_CACHE_POLICY))
}

pub fn url_cache_policy() -> UrlCachePolicy {
    *policy_cell().read()
}

pub fn set_url_cache_policy(policy: UrlCachePolicy) {
    *policy_cell().write() = policy.sanitized();
}

pub fn set_url_cache_seconds(seconds: u64) {
    policy_cell().write().seconds = seconds;
}

pub fn set_url_cache_verify(verify: bool) {
    policy_cell().write().verify = verify;
}

pub fn set_url_cache_renewals(renewals: u32) {
    policy_cell().write().renewals = renewals;
}

pub fn set_url_cache_probe_percent(percent: u8) {
    policy_cell().write().probe_percent =
        percent.clamp(MIN_URL_CACHE_PROBE_PERCENT, MAX_URL_CACHE_PROBE_PERCENT);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheLookup {
    Fresh(DedupEntry),
    Probe(DedupEntry),
    Stale(DedupEntry),
    Miss,
}

impl CacheLookup {
    pub fn usable(&self) -> Option<DedupEntry> {
        match self {
            CacheLookup::Fresh(entry) | CacheLookup::Probe(entry) => Some(entry.clone()),
            CacheLookup::Stale(_) | CacheLookup::Miss => None,
        }
    }
}

struct Slot {
    entry: DedupEntry,
    stored_ms: u64,
    probe_ms: u64,
    renewals: u32,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<String, Slot>,
    order: VecDeque<String>,
}

impl Inner {
    fn touch(&mut self, key: &str) {
        self.order.retain(|item| item != key);
        self.order.push_back(key.to_string());
    }

    fn drop_entry(&mut self, key: &str) {
        self.entries.remove(key);
        self.order.retain(|item| item != key);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

pub struct UploadDedupCache {
    capacity: usize,
    inner: Mutex<Inner>,
    policy: RwLock<Option<UrlCachePolicy>>,
}

impl Default for UploadDedupCache {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl UploadDedupCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: Mutex::new(Inner::default()),
            policy: RwLock::new(None),
        }
    }

    pub fn with_policy(capacity: usize, policy: UrlCachePolicy) -> Self {
        let cache = Self::new(capacity);
        cache.set_policy(policy);
        cache
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn policy(&self) -> UrlCachePolicy {
        self.effective_policy()
    }

    pub fn set_policy(&self, policy: UrlCachePolicy) {
        *self.policy.write() = Some(policy.sanitized());
    }

    pub fn len(&self) -> usize {
        self.inner.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, key: &str) -> Option<DedupEntry> {
        self.lookup(key).usable()
    }

    pub fn lookup(&self, key: &str) -> CacheLookup {
        self.lookup_at(key, now_ms())
    }

    pub fn put(&self, key: impl Into<String>, url: impl Into<String>, provider: impl Into<String>) {
        let policy = self.effective_policy();
        if self.capacity == 0 || policy.seconds == 0 {
            return;
        }
        let key = key.into();
        let stored_ms = now_ms();
        let entry = DedupEntry {
            url: url.into(),
            provider: provider.into(),
        };
        let mut inner = self.inner.lock();
        inner.entries.insert(
            key.clone(),
            Slot {
                entry,
                stored_ms,
                probe_ms: stored_ms,
                renewals: 0,
            },
        );
        inner.touch(&key);
        while inner.entries.len() > self.capacity {
            let Some(evicted) = inner.order.pop_front() else {
                break;
            };
            inner.entries.remove(&evicted);
        }
    }

    pub fn renew(&self, key: &str) -> bool {
        self.renew_at(key, now_ms())
    }

    pub fn invalidate(&self, key: &str) {
        self.inner.lock().drop_entry(key);
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.entries.clear();
        inner.order.clear();
    }

    fn effective_policy(&self) -> UrlCachePolicy {
        match *self.policy.read() {
            Some(policy) => policy,
            None => url_cache_policy(),
        }
    }

    fn lookup_at(&self, key: &str, at_ms: u64) -> CacheLookup {
        let policy = self.effective_policy();
        if self.capacity == 0 || policy.seconds == 0 {
            return CacheLookup::Miss;
        }
        let lifetime = policy.lifetime_ms();
        let mut inner = self.inner.lock();
        let Some(slot) = inner.entries.get(key) else {
            return CacheLookup::Miss;
        };
        let entry = slot.entry.clone();
        let age = at_ms.saturating_sub(slot.stored_ms);
        if age >= lifetime {
            return CacheLookup::Stale(entry);
        }
        let probe_due = policy.verify
            && age >= lifetime / 2
            && at_ms.saturating_sub(slot.probe_ms) >= policy.probe_interval_ms();
        if probe_due {
            if let Some(slot) = inner.entries.get_mut(key) {
                slot.probe_ms = at_ms;
            }
            inner.touch(key);
            return CacheLookup::Probe(entry);
        }
        inner.touch(key);
        CacheLookup::Fresh(entry)
    }

    fn renew_at(&self, key: &str, at_ms: u64) -> bool {
        let policy = self.effective_policy();
        if policy.seconds == 0 {
            self.invalidate(key);
            return false;
        }
        let mut inner = self.inner.lock();
        let Some(slot) = inner.entries.get(key) else {
            return false;
        };
        if slot.renewals >= policy.renewals {
            inner.drop_entry(key);
            return false;
        }
        if let Some(slot) = inner.entries.get_mut(key) {
            slot.renewals += 1;
            slot.stored_ms = at_ms;
            slot.probe_ms = at_ms;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_back_entries() {
        let cache = UploadDedupCache::with_policy(4, UrlCachePolicy::default());
        assert!(cache.is_empty());
        cache.put("k1", "https://cdn/1.png", "sgame");
        cache.put("k2", "https://cdn/2.png", "cnb");
        assert_eq!(cache.len(), 2);
        let hit = cache.get("k1").expect("应当命中");
        assert_eq!(hit.url, "https://cdn/1.png");
        assert_eq!(hit.provider, "sgame");
        assert!(cache.get("missing").is_none());
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn evicts_least_recently_used() {
        let cache = UploadDedupCache::with_policy(2, UrlCachePolicy::default());
        cache.put("a", "https://cdn/a", "p");
        cache.put("b", "https://cdn/b", "p");
        assert!(cache.get("a").is_some());
        cache.put("c", "https://cdn/c", "p");
        assert_eq!(cache.len(), 2);
        assert!(cache.get("a").is_some());
        assert!(cache.get("b").is_none());
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn zero_capacity_never_stores() {
        let cache = UploadDedupCache::with_policy(0, UrlCachePolicy::default());
        cache.put("a", "https://cdn/a", "p");
        assert_eq!(cache.capacity(), 0);
        assert!(cache.is_empty());
        assert!(cache.get("a").is_none());
    }

    #[test]
    fn default_capacity_matches_go_independent_choice() {
        let cache = UploadDedupCache::default();
        assert_eq!(cache.capacity(), DEFAULT_CAPACITY);
    }

    #[test]
    fn ttl_verify_renew_and_probe_follow_the_policy() {
        let policy = UrlCachePolicy::new(100, true, 2, 50);
        let cache = UploadDedupCache::with_policy(8, policy);
        let start = now_ms();
        cache.put("k", "https://cdn/k.png", "p");
        assert!(matches!(
            cache.lookup_at("k", start),
            CacheLookup::Fresh(_)
        ));
        assert!(
            matches!(cache.lookup_at("k", start + 60_000), CacheLookup::Probe(_)),
            "过半个寿命后命中必须先回验"
        );
        assert!(
            matches!(cache.lookup_at("k", start + 70_000), CacheLookup::Fresh(_)),
            "探活比例之内不重复回验"
        );
        assert!(
            matches!(cache.lookup_at("k", start + 120_000), CacheLookup::Stale(_)),
            "超过寿命必须判为过期"
        );
        assert!(cache.renew_at("k", start + 120_000), "到期探活通过后应当续期");
        assert!(matches!(
            cache.lookup_at("k", start + 120_000),
            CacheLookup::Fresh(_)
        ));
        assert!(cache.renew_at("k", start + 120_000));
        assert!(!cache.renew_at("k", start + 120_000), "续期次数用尽后不再续");
        assert!(matches!(
            cache.lookup_at("k", start + 120_000),
            CacheLookup::Miss
        ));
    }

    #[test]
    fn zero_ttl_disables_the_layer() {
        let cache = UploadDedupCache::with_policy(8, UrlCachePolicy::new(0, true, 6, 25));
        cache.put("k", "https://cdn/k.png", "p");
        assert!(cache.is_empty(), "关掉这一层之后不该再存");
        assert!(cache.get("k").is_none());
    }

    #[test]
    fn global_policy_setters_take_effect_immediately() {
        set_url_cache_policy(UrlCachePolicy::default());
        let cache = UploadDedupCache::new(8);
        cache.put("k", "https://cdn/k.png", "p");
        assert!(cache.get("k").is_some(), "默认策略下应当命中");

        set_url_cache_verify(false);
        assert!(!url_cache_policy().verify);
        set_url_cache_renewals(2);
        assert_eq!(url_cache_policy().renewals, 2);
        set_url_cache_probe_percent(200);
        assert_eq!(url_cache_policy().probe_percent, MAX_URL_CACHE_PROBE_PERCENT);
        set_url_cache_seconds(0);
        assert_eq!(url_cache_policy().seconds, 0);
        assert!(cache.get("k").is_none(), "关掉这一层之后不能再命中");

        set_url_cache_seconds(DEFAULT_URL_CACHE_SECONDS);
        assert!(cache.get("k").is_some(), "重新打开之后旧条目仍然可用");
        set_url_cache_policy(UrlCachePolicy::default());
        assert_eq!(url_cache_policy(), UrlCachePolicy::default());
    }
}
