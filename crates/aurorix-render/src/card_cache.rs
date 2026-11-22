use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use aurorix_core::{check_memory_pressure, HardwareProfile, HardwareTier};
use bytes::Bytes;
use tokio::sync::OnceCell;

use crate::RenderError;

fn budget() -> usize {
    if let Some(bytes) = crate::settings::render_budget_bytes() {
        return usize::try_from(bytes).unwrap_or(usize::MAX);
    }
    match HardwareProfile::cached().tier() {
        HardwareTier::Nano => 8 << 20,
        HardwareTier::Compact => 24 << 20,
        HardwareTier::Balanced => 64 << 20,
        HardwareTier::Performance => 128 << 20,
    }
}

const PRESSURE_KEEP_PERCENT: usize = 75;

static HITS: AtomicU64 = AtomicU64::new(0);

static MISSES: AtomicU64 = AtomicU64::new(0);

static RENDERS: AtomicU64 = AtomicU64::new(0);

struct Slot {
    bytes: Bytes,
    seen: u64,
}

type Flight = Arc<OnceCell<Result<Bytes, RenderError>>>;

struct Cache {
    entries: HashMap<u64, Slot>,
    inflight: HashMap<u64, Flight>,
    used: usize,
    clock: u64,
    budget: usize,
}

impl Cache {
    fn new() -> Self {
        Self::with_budget(budget())
    }

    fn with_budget(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            inflight: HashMap::new(),
            used: 0,
            clock: 0,
            budget,
        }
    }

    fn get(&mut self, key: u64) -> Option<Bytes> {
        self.clock += 1;
        let clock = self.clock;
        let slot = self.entries.get_mut(&key)?;
        slot.seen = clock;
        Some(slot.bytes.clone())
    }

    fn put(&mut self, key: u64, bytes: Bytes) {
        let size = bytes.len();
        if size == 0 || size > self.budget {
            return;
        }
        self.clock += 1;
        if let Some(previous) = self.entries.remove(&key) {
            self.used -= previous.bytes.len();
        }
        while self.used + size > self.budget {
            let before = self.entries.len();
            self.evict_oldest();
            if self.entries.len() == before {
                break;
            }
        }
        self.used += size;
        self.entries.insert(
            key,
            Slot {
                bytes,
                seen: self.clock,
            },
        );
    }

    fn evict_oldest(&mut self) {
        let Some(victim) = self
            .entries
            .iter()
            .min_by_key(|(_, slot)| slot.seen)
            .map(|(key, _)| *key)
        else {
            return;
        };
        if let Some(removed) = self.entries.remove(&victim) {
            self.used -= removed.bytes.len();
        }
    }

    fn shrink(&mut self) {
        let target = self.budget / 100 * PRESSURE_KEEP_PERCENT;
        while self.used > target {
            let before = self.entries.len();
            self.evict_oldest();
            if self.entries.len() == before {
                break;
            }
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.used = 0;
    }

    fn flight(&mut self, key: u64) -> Flight {
        Arc::clone(
            self.inflight
                .entry(key)
                .or_insert_with(|| Arc::new(OnceCell::new())),
        )
    }
}

fn shared() -> &'static Mutex<Cache> {
    static CARD_BYTES_MEMO: OnceLock<Mutex<Cache>> = OnceLock::new();
    CARD_BYTES_MEMO.get_or_init(|| Mutex::new(Cache::new()))
}

fn lock() -> MutexGuard<'static, Cache> {
    shared()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn hits() -> u64 {
    HITS.load(Ordering::Relaxed)
}

pub fn misses() -> u64 {
    MISSES.load(Ordering::Relaxed)
}

pub fn renders() -> u64 {
    RENDERS.load(Ordering::Relaxed)
}

pub fn budget_bytes() -> usize {
    budget()
}

pub fn used_bytes() -> usize {
    lock().used
}

pub fn entries() -> usize {
    lock().entries.len()
}

pub fn clear() {
    lock().clear();
}

pub fn peek(key: u64) -> Option<Bytes> {
    let hit = lock().get(key);
    if hit.is_some() {
        HITS.fetch_add(1, Ordering::Relaxed);
    }
    hit
}

pub fn store(key: u64, bytes: Bytes) {
    let high = check_memory_pressure().is_high();
    let mut cache = lock();
    cache.budget = budget();
    if high {
        cache.shrink();
        return;
    }
    cache.put(key, bytes);
}

pub async fn cached<F>(key: u64, render: F) -> Result<Bytes, RenderError>
where
    F: Future<Output = Result<Vec<u8>, RenderError>>,
{
    if let Some(hit) = peek(key) {
        return Ok(hit);
    }
    let cell = lock().flight(key);
    let outcome = cell
        .get_or_init(|| async {
            RENDERS.fetch_add(1, Ordering::Relaxed);
            render.await.map(Bytes::from)
        })
        .await;
    let result = match outcome {
        Ok(bytes) => {
            MISSES.fetch_add(1, Ordering::Relaxed);
            store(key, bytes.clone());
            Ok(bytes.clone())
        }
        Err(error) => Err(error.clone()),
    };
    lock().inflight.remove(&key);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(size: usize, seed: u8) -> Bytes {
        Bytes::from(vec![seed; size])
    }

    #[test]
    fn put_evicts_until_within_budget() {
        let mut cache = Cache::with_budget(600);
        cache.put(1, payload(300, 1));
        cache.put(2, payload(300, 2));
        cache.put(3, payload(300, 3));
        assert_eq!(cache.used, 600, "写入后占用必须回到额度以内");
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.get(1).is_none(), "最旧的一条应当先被淘汰");
        assert!(cache.get(3).is_some());
    }

    #[test]
    fn shrink_keeps_recent_entries_instead_of_clearing() {
        let mut cache = Cache::with_budget(1000);
        for key in 1..=3u64 {
            cache.put(key, payload(300, key as u8));
        }
        assert_eq!(cache.used, 900);
        cache.shrink();
        assert!(!cache.entries.is_empty(), "高压时不允许整表清空");
        assert_eq!(cache.entries.len(), 2, "只该淘汰到水位线以下");
        assert_eq!(cache.used, 600);
        assert!(cache.get(1).is_none(), "最先淘汰的应当是最旧的一条");
        assert!(cache.get(2).is_some(), "较新的条目必须留着");
        assert!(cache.get(3).is_some(), "最新的条目必须留着");
    }
}
