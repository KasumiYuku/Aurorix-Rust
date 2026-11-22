use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwapOption;
use tokio::sync::Mutex;

use crate::admin::{BanEntry, BanStore};
use crate::error::DbResult;

pub type CacheFuture<'a, T> = Pin<Box<dyn Future<Output = DbResult<T>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StalePolicy {
    ServeStale,
    Propagate,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub loads: u64,
    pub hits: u64,
    pub misses: u64,
    pub stale_hits: u64,
}

pub struct HotCache<V: Send + Sync + 'static> {
    fresh: ArcSwapOption<V>,
    stale: ArcSwapOption<V>,
    gate: Mutex<()>,
    policy: StalePolicy,
    loads: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    stale_hits: AtomicU64,
}

impl<V: Send + Sync + 'static> std::fmt::Debug for HotCache<V> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HotCache")
            .field("loaded", &self.is_loaded())
            .field("policy", &self.policy)
            .field("stats", &self.stats())
            .finish()
    }
}

impl<V: Send + Sync + 'static> Default for HotCache<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Send + Sync + 'static> HotCache<V> {
    pub fn new() -> Self {
        Self::with_policy(StalePolicy::ServeStale)
    }

    pub fn with_policy(policy: StalePolicy) -> Self {
        Self {
            fresh: ArcSwapOption::empty(),
            stale: ArcSwapOption::empty(),
            gate: Mutex::new(()),
            policy,
            loads: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            stale_hits: AtomicU64::new(0),
        }
    }

    pub fn policy(&self) -> StalePolicy {
        self.policy
    }

    pub fn is_loaded(&self) -> bool {
        self.fresh.load().is_some()
    }

    pub fn get(&self) -> Option<Arc<V>> {
        self.fresh.load_full()
    }

    pub fn store(&self, value: Arc<V>) {
        self.stale.store(Some(Arc::clone(&value)));
        self.fresh.store(Some(value));
    }

    pub fn invalidate(&self) {
        self.fresh.store(None);
    }

    pub fn clear_stale(&self) {
        self.stale.store(None);
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            loads: self.loads.load(Ordering::Relaxed),
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            stale_hits: self.stale_hits.load(Ordering::Relaxed),
        }
    }

    pub async fn load_or<F, Fut>(&self, loader: F) -> DbResult<Arc<V>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = DbResult<Arc<V>>>,
    {
        if let Some(hit) = self.get() {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(hit);
        }
        let _gate = self.gate.lock().await;
        if let Some(hit) = self.get() {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(hit);
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        self.loads.fetch_add(1, Ordering::Relaxed);
        match loader().await {
            Ok(value) => {
                self.store(Arc::clone(&value));
                Ok(value)
            }
            Err(err) => match self.stale.load_full() {
                Some(stale) if self.policy == StalePolicy::ServeStale => {
                    self.stale_hits.fetch_add(1, Ordering::Relaxed);
                    self.fresh.store(Some(Arc::clone(&stale)));
                    Ok(stale)
                }
                _ => Err(err),
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterView {
    entries: HashMap<String, Arc<BanEntry>>,
}

impl RosterView {
    pub fn from_entries(entries: impl IntoIterator<Item = BanEntry>) -> Self {
        let mut view = Self::default();
        for entry in entries {
            if entry.openid.trim().is_empty() {
                continue;
            }
            view.entries.insert(entry.openid.clone(), Arc::new(entry));
        }
        view
    }

    pub fn contains(&self, openid: &str) -> bool {
        self.entries.contains_key(openid)
    }

    pub fn get(&self, openid: &str) -> Option<&Arc<BanEntry>> {
        self.entries.get(openid)
    }

    pub fn text_of(&self, openid: &str) -> Option<&str> {
        self.entries.get(openid).map(|entry| entry.text())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &Arc<BanEntry>> {
        self.entries.values()
    }

    pub fn openids(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }
}

pub trait RosterSource: Send + Sync {
    fn roster(&self) -> CacheFuture<'_, RosterView>;
}

impl RosterSource for BanStore {
    fn roster(&self) -> CacheFuture<'_, RosterView> {
        Box::pin(async move { Ok(RosterView::from_entries(self.load().await?)) })
    }
}

impl RosterSource for Arc<BanStore> {
    fn roster(&self) -> CacheFuture<'_, RosterView> {
        Box::pin(async move { Ok(RosterView::from_entries(self.load().await?)) })
    }
}

pub struct BanCache<S: RosterSource = BanStore> {
    source: Arc<S>,
    cache: HotCache<RosterView>,
}

impl<S: RosterSource> std::fmt::Debug for BanCache<S> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BanCache")
            .field("loaded", &self.cache.is_loaded())
            .field("stats", &self.cache.stats())
            .finish()
    }
}

impl<S: RosterSource> BanCache<S> {
    pub fn new(source: S) -> Self {
        Self::with_policy(source, StalePolicy::ServeStale)
    }

    pub fn with_policy(source: S, policy: StalePolicy) -> Self {
        Self {
            source: Arc::new(source),
            cache: HotCache::with_policy(policy),
        }
    }

    pub fn from_shared(source: Arc<S>) -> Self {
        Self {
            source,
            cache: HotCache::new(),
        }
    }

    pub fn source(&self) -> &Arc<S> {
        &self.source
    }

    pub fn cached(&self) -> Option<Arc<RosterView>> {
        self.cache.get()
    }

    pub fn stats(&self) -> CacheStats {
        self.cache.stats()
    }

    pub fn invalidate(&self) {
        self.cache.invalidate();
    }

    pub async fn view(&self) -> DbResult<Arc<RosterView>> {
        let source = Arc::clone(&self.source);
        self.cache
            .load_or(move || async move { source.roster().await.map(Arc::new) })
            .await
    }

    pub async fn refresh(&self) -> DbResult<Arc<RosterView>> {
        self.invalidate();
        self.view().await
    }

    pub async fn is_banned(&self, openid: &str) -> DbResult<bool> {
        if openid.is_empty() {
            return Ok(false);
        }
        Ok(self.view().await?.contains(openid))
    }

    pub async fn ban_message(&self, openid: &str) -> DbResult<Option<String>> {
        if openid.is_empty() {
            return Ok(None);
        }
        Ok(self
            .view()
            .await?
            .text_of(openid)
            .map(|text| text.to_string()))
    }
}

impl BanCache<BanStore> {
    pub fn from_store(store: BanStore) -> Self {
        Self::new(store)
    }

    pub async fn add_entry(&self, entry: BanEntry) -> DbResult<BanEntry> {
        let saved = self.source.add(entry).await?;
        self.refresh().await?;
        Ok(saved)
    }

    pub async fn remove_entry(&self, openid: &str) -> DbResult<()> {
        self.source.remove(openid).await?;
        self.refresh().await?;
        Ok(())
    }

    pub async fn clear_entries(&self) -> DbResult<()> {
        self.source.clear().await?;
        self.refresh().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use super::*;

    struct CountingSource {
        calls: Arc<AtomicUsize>,
        value: usize,
        fail: Arc<AtomicUsize>,
    }

    impl CountingSource {
        fn new(value: usize) -> (Self, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    calls: Arc::clone(&calls),
                    value,
                    fail: Arc::new(AtomicUsize::new(0)),
                },
                calls,
            )
        }

        fn failing(value: usize) -> (Self, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    calls: Arc::clone(&calls),
                    value,
                    fail: Arc::new(AtomicUsize::new(1)),
                },
                calls,
            )
        }
    }

    impl RosterSource for CountingSource {
        fn roster(&self) -> CacheFuture<'_, RosterView> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if self.fail.load(Ordering::SeqCst) > 0 {
                    self.fail.fetch_sub(1, Ordering::SeqCst);
                    return Err(crate::error::DbError::Invalid("模拟读取失败".to_string()));
                }
                let entry = BanEntry::new(format!("openid-{}", self.value));
                Ok(RosterView::from_entries(vec![entry]))
            })
        }
    }

    fn entry(openid: &str) -> BanEntry {
        BanEntry::new(openid).with_message("被封禁了")
    }

    #[tokio::test]
    async fn hit_path_never_calls_the_loader_again() {
        let (source, calls) = CountingSource::new(1);
        let cache = BanCache::new(source);
        let first = cache.view().await.expect("首次装载");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(first.contains("openid-1"));
        for _ in 0..32 {
            assert!(cache.cached().is_some(), "热路径必须命中");
            assert!(cache.view().await.expect("命中").contains("openid-1"));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "命中不该再查一次");
        assert_eq!(cache.stats().loads, 1);
        assert_eq!(cache.stats().hits, 32);
    }

    #[tokio::test]
    async fn concurrent_readers_share_a_single_load() {
        let (source, calls) = CountingSource::new(7);
        let cache = Arc::new(BanCache::new(source));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let cache = Arc::clone(&cache);
            handles.push(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(5)).await;
                cache.view().await.expect("装载").contains("openid-7")
            }));
        }
        for handle in handles {
            assert!(handle.await.expect("任务"));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "单飞: 只允许一次加载");
        assert_eq!(cache.stats().loads, 1);
    }

    #[tokio::test]
    async fn invalidation_forces_exactly_one_reload() {
        let (source, calls) = CountingSource::new(3);
        let cache = BanCache::new(source);
        cache.view().await.expect("首次装载");
        cache.invalidate();
        assert!(cache.cached().is_none(), "失效后不该再有新鲜值");
        cache.view().await.expect("重新装载");
        assert_eq!(calls.load(Ordering::SeqCst), 2, "只允许重载一次");
        cache.view().await.expect("命中");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn stale_policy_serves_last_good_value_on_error() {
        let (source, calls) = CountingSource::failing(5);
        source.fail.store(0, Ordering::SeqCst);
        let cache = BanCache::with_policy(source, StalePolicy::ServeStale);
        cache.view().await.expect("首次装载");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        cache.invalidate();
        cache.source().fail.store(1, Ordering::SeqCst);
        let served = cache.view().await.expect("应当返回旧值");
        assert!(served.contains("openid-5"));
        assert_eq!(cache.stats().stale_hits, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn propagate_policy_surfaces_the_error() {
        let (source, _calls) = CountingSource::new(9);
        let cache = BanCache::with_policy(source, StalePolicy::Propagate);
        cache.view().await.expect("首次装载");
        cache.invalidate();
        cache.source().fail.store(1, Ordering::SeqCst);
        let err = cache.view().await.expect_err("必须把错误抛出去");
        assert!(err.to_string().contains("模拟读取失败"));
        assert_eq!(cache.stats().stale_hits, 0);
    }

    #[tokio::test]
    async fn roster_view_lookup_is_identity_based_and_constant_time() {
        let view = RosterView::from_entries(vec![
            entry("u1").with_message("一号"),
            BanEntry::new("u2"),
            BanEntry::default(),
        ]);
        assert!(view.contains("u1"));
        assert!(view.contains("u2"));
        assert!(!view.contains("u3"));
        assert!(!view.contains(""));
        assert_eq!(view.len(), 2, "空 openid 不该进名单");
        assert_eq!(view.text_of("u1"), Some("一号"));
        assert_eq!(
            view.text_of("u2"),
            Some(crate::admin::DEFAULT_BAN_MESSAGE),
            "没写文案时用默认文案"
        );
        assert_eq!(view.text_of("u3"), None);
        assert!(!view.is_empty());
        assert!(RosterView::default().is_empty());
    }

    #[tokio::test]
    async fn ban_message_reads_through_the_cache() {
        let (source, calls) = CountingSource::new(11);
        let cache = BanCache::new(source);
        assert_eq!(
            cache.ban_message("openid-11").await.expect("读"),
            Some(crate::admin::DEFAULT_BAN_MESSAGE.to_string()),
            "没有自定义文案时回退到默认封禁文案"
        );
        assert_eq!(cache.ban_message("other").await.expect("读"), None);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn hot_cache_store_and_get_are_immediate() {
        let cache: HotCache<String> = HotCache::new();
        assert!(cache.get().is_none());
        cache.store(Arc::new("v1".to_string()));
        assert_eq!(cache.get().as_deref(), Some(&"v1".to_string()));
        cache.invalidate();
        assert!(cache.get().is_none());
        assert!(cache
            .load_or(|| async { Ok(Arc::new("v2".to_string())) })
            .await
            .is_ok());
        assert_eq!(cache.stats().loads, 1);
    }
}
