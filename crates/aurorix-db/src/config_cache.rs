use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::error::{DbError, DbResult};
use crate::kv::KvStore;

pub const CONFIG_NAMESPACE: &str = "config";

pub const KV_SCAN: &str =
    "SELECT key, value FROM kv_data WHERE scope = $1 AND namespace = $2 ORDER BY key";

pub const DEFAULT_CONFIG_LIMIT_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub reloads: u64,
    pub entries: usize,
    pub loaded: bool,
}

#[derive(Debug)]
pub struct ConfigCache {
    store: KvStore,
    entries: ArcSwap<HashMap<String, Arc<Value>>>,
    hits: AtomicU64,
    misses: AtomicU64,
    reloads: AtomicU64,
    limit_bytes: usize,
}

impl ConfigCache {
    pub fn new(store: KvStore) -> Self {
        Self {
            store,
            entries: ArcSwap::from_pointee(HashMap::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            reloads: AtomicU64::new(0),
            limit_bytes: DEFAULT_CONFIG_LIMIT_BYTES,
        }
    }

    pub fn with_limit(mut self, limit_bytes: usize) -> Self {
        self.limit_bytes = limit_bytes.max(1);
        self
    }

    pub fn global(store: KvStore) -> Self {
        Self::new(store)
    }

    pub fn namespace(&self) -> &str {
        self.store.namespace()
    }

    pub fn scope(&self) -> &str {
        self.store.scope()
    }

    pub fn limit_bytes(&self) -> usize {
        self.limit_bytes
    }

    pub fn is_loaded(&self) -> bool {
        self.reloads.load(Ordering::Acquire) > 0
    }

    pub fn len(&self) -> usize {
        self.entries.load().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.load().is_empty()
    }

    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.entries.load().keys().cloned().collect();
        keys.sort();
        keys
    }

    pub fn stats(&self) -> ConfigCacheStats {
        ConfigCacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            reloads: self.reloads.load(Ordering::Relaxed),
            entries: self.entries.load().len(),
            loaded: self.is_loaded(),
        }
    }

    pub async fn load(&self) -> DbResult<usize> {
        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(KV_SCAN)
            .bind(self.store.scope())
            .bind(self.store.namespace())
            .fetch_all(self.store.pool())
            .await?;
        let mut fresh: HashMap<String, Arc<Value>> = HashMap::with_capacity(rows.len());
        let mut used = 0usize;
        for (key, encoded) in rows {
            used = used.saturating_add(key.len() + encoded.len());
            if used > self.limit_bytes {
                break;
            }
            match serde_json::from_slice::<Value>(&encoded) {
                Ok(value) => {
                    fresh.insert(key, Arc::new(value));
                }
                Err(_) => continue,
            }
        }
        let count = fresh.len();
        self.entries.store(Arc::new(fresh));
        self.reloads.fetch_add(1, Ordering::Release);
        Ok(count)
    }

    pub async fn reload(&self) -> DbResult<usize> {
        self.load().await
    }

    pub fn get_raw(&self, key: &str) -> Option<Arc<Value>> {
        let found = self.entries.load().get(key).map(Arc::clone);
        match &found {
            Some(_) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
            }
        }
        found
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> DbResult<Option<T>> {
        if let Some(value) = self.get_raw(key) {
            return serde_json::from_value((*value).clone())
                .map(Some)
                .map_err(|err| DbError::Decode(err.to_string()));
        }
        let fetched: Option<Value> = self.store.get(key).await?;
        match fetched {
            Some(raw) => {
                self.insert_shared(key, Arc::new(raw.clone()));
                serde_json::from_value(raw)
                    .map(Some)
                    .map_err(|err| DbError::Decode(err.to_string()))
            }
            None => Ok(None),
        }
    }

    pub async fn get_or<T: DeserializeOwned>(&self, key: &str, fallback: T) -> DbResult<T> {
        Ok(self.get(key).await?.unwrap_or(fallback))
    }

    pub async fn set<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> DbResult<()> {
        self.store.set(key, value).await?;
        if let Ok(raw) = serde_json::to_value(value) {
            self.insert_shared(key, Arc::new(raw));
        } else {
            self.invalidate(key);
        }
        Ok(())
    }

    pub async fn set_raw(&self, key: &str, value: Value) -> DbResult<()> {
        self.store.set(key, &value).await?;
        self.insert_shared(key, Arc::new(value));
        Ok(())
    }

    pub async fn delete(&self, key: &str) -> DbResult<()> {
        self.store.delete(key).await?;
        self.invalidate(key);
        Ok(())
    }

    pub fn invalidate(&self, key: &str) {
        let current = self.entries.load();
        if !current.contains_key(key) {
            return;
        }
        let mut fresh: HashMap<String, Arc<Value>> = (**current).clone();
        fresh.remove(key);
        self.entries.store(Arc::new(fresh));
    }

    pub fn invalidate_all(&self) {
        self.entries.store(Arc::new(HashMap::new()));
    }

    pub fn insert_shared(&self, key: &str, value: Arc<Value>) {
        let current = self.entries.load();
        if current
            .get(key)
            .is_some_and(|known| Arc::ptr_eq(known, &value))
        {
            return;
        }
        let mut fresh: HashMap<String, Arc<Value>> = (**current).clone();
        fresh.insert(key.to_string(), value);
        self.entries.store(Arc::new(fresh));
    }

    pub fn seed(&self, entries: impl IntoIterator<Item = (String, Value)>) {
        let mut fresh: HashMap<String, Arc<Value>> = (**self.entries.load()).clone();
        for (key, value) in entries {
            fresh.insert(key, Arc::new(value));
        }
        self.entries.store(Arc::new(fresh));
        self.reloads.fetch_add(1, Ordering::Release);
    }

    pub fn store_raw(&self, key: &str, value: Value) {
        self.insert_shared(key, Arc::new(value));
    }
}
