use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::Mutex;
use skia_safe::{Data, Image};

use crate::error::RenderError;
use crate::handle::SharedImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: u64,
    pub hits: u64,
    pub misses: u64,
    pub evicted: u64,
    pub decodes: u64,
}

pub struct Texture {
    key: Arc<str>,
    encoded: Arc<Bytes>,
    image: Option<Arc<SharedImage>>,
}

impl Texture {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn len(&self) -> usize {
        self.encoded.len()
    }

    pub fn is_empty(&self) -> bool {
        self.encoded.is_empty()
    }

    pub fn encoded(&self) -> &Bytes {
        &self.encoded
    }

    pub fn image(&self) -> Option<&Image> {
        self.image.as_deref().map(SharedImage::image)
    }

    pub fn decoded(&self) -> Option<Arc<SharedImage>> {
        self.image.clone()
    }

    pub fn dimensions(&self) -> Option<(i32, i32)> {
        self.image.as_ref().map(|image| image.dimensions())
    }
}

impl std::fmt::Debug for Texture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Texture")
            .field("key", &self.key)
            .field("bytes", &self.encoded.len())
            .field("dimensions", &self.dimensions())
            .finish()
    }
}

struct TextureState {
    entries: HashMap<Arc<str>, Arc<Texture>>,
    order: VecDeque<Arc<str>>,
    bytes: u64,
}

impl TextureState {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
        }
    }

    fn forget(&mut self, key: &str) -> Option<Arc<Texture>> {
        let removed = self.entries.remove(key)?;
        self.bytes -= removed.len() as u64;
        self.order.retain(|tracked| &**tracked != key);
        Some(removed)
    }

    fn touch(&mut self, key: &str) {
        self.order.retain(|tracked| &**tracked != key);
        self.order.push_back(Arc::from(key));
    }
}

pub struct TextureCache {
    capacity_bytes: u64,
    state: Mutex<TextureState>,
    hits: AtomicU64,
    misses: AtomicU64,
    evicted: AtomicU64,
    decodes: AtomicU64,
}

impl TextureCache {
    pub fn new(capacity_bytes: u64) -> Self {
        Self {
            capacity_bytes,
            state: Mutex::new(TextureState::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evicted: AtomicU64::new(0),
            decodes: AtomicU64::new(0),
        }
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.capacity_bytes
    }

    pub fn insert(&self, key: &str, bytes: Bytes) -> Result<Arc<Texture>, RenderError> {
        self.store(Texture {
            key: Arc::from(key),
            encoded: Arc::new(bytes),
            image: None,
        })
    }

    pub fn insert_encoded(&self, key: &str, bytes: Bytes) -> Result<Arc<Texture>, RenderError> {
        let image = decode(key, &bytes)?;
        self.decodes.fetch_add(1, Ordering::SeqCst);
        self.store(Texture {
            key: Arc::from(key),
            encoded: Arc::new(bytes),
            image: Some(Arc::new(image)),
        })
    }

    pub fn image_for(&self, bytes: Bytes) -> Result<Arc<Texture>, RenderError> {
        let key = content_key(&bytes);
        if let Some(hit) = self.get(&key) {
            if hit.image().is_some() {
                return Ok(hit);
            }
            self.remove(&key);
        }
        self.insert_encoded(&key, bytes)
    }

    pub fn decodes(&self) -> u64 {
        self.decodes.load(Ordering::SeqCst)
    }

    pub fn drop_decoded(&self) -> usize {
        let mut state = self.state.lock();
        let mut dropped = 0usize;
        let keys: Vec<Arc<str>> = state.entries.keys().cloned().collect();
        for key in keys {
            let Some(entry) = state.entries.get(&key) else {
                continue;
            };
            if entry.image.is_none() {
                continue;
            }
            let stripped = Texture {
                key: Arc::clone(&entry.key),
                encoded: Arc::clone(&entry.encoded),
                image: None,
            };
            state.entries.insert(Arc::clone(&key), Arc::new(stripped));
            dropped += 1;
        }
        dropped
    }

    pub fn get(&self, key: &str) -> Option<Arc<Texture>> {
        let mut state = self.state.lock();
        match state.entries.get(key).cloned() {
            Some(found) => {
                state.touch(key);
                self.hits.fetch_add(1, Ordering::SeqCst);
                Some(found)
            }
            None => {
                self.misses.fetch_add(1, Ordering::SeqCst);
                None
            }
        }
    }

    pub fn remove(&self, key: &str) {
        let mut state = self.state.lock();
        state.forget(key);
    }

    pub fn invalidate_all(&self) {
        let mut state = self.state.lock();
        state.entries.clear();
        state.order.clear();
        state.bytes = 0;
    }

    pub fn stats(&self) -> CacheStats {
        let state = self.state.lock();
        CacheStats {
            entries: state.entries.len(),
            bytes: state.bytes,
            hits: self.hits.load(Ordering::SeqCst),
            misses: self.misses.load(Ordering::SeqCst),
            evicted: self.evicted.load(Ordering::SeqCst),
            decodes: self.decodes.load(Ordering::SeqCst),
        }
    }

    fn store(&self, texture: Texture) -> Result<Arc<Texture>, RenderError> {
        let incoming = texture.len() as u64;
        if incoming > self.capacity_bytes {
            return Err(RenderError::Texture(format!(
                "纹理 {} 需要 {incoming} 字节, 超过缓存容量 {} 字节",
                texture.key, self.capacity_bytes
            )));
        }
        let stored = Arc::new(texture);
        let mut state = self.state.lock();
        state.forget(&stored.key);
        state.bytes += incoming;
        state
            .entries
            .insert(Arc::clone(&stored.key), Arc::clone(&stored));
        state.order.push_back(Arc::clone(&stored.key));
        while state.bytes > self.capacity_bytes {
            let Some(oldest) = state.order.front().cloned() else {
                break;
            };
            state.forget(&oldest);
            self.evicted.fetch_add(1, Ordering::SeqCst);
        }
        Ok(stored)
    }
}

fn decode(key: &str, bytes: &[u8]) -> Result<SharedImage, RenderError> {
    let decoded = Image::from_encoded(Data::new_copy(bytes)).ok_or_else(|| {
        RenderError::Texture(format!("纹理 {key} 的 {} 字节无法解码为位图", bytes.len()))
    })?;
    Ok(SharedImage::from_image(decoded))
}

pub fn content_key(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for chunk in bytes.chunks(8) {
        let mut lane = [0u8; 8];
        lane[..chunk.len()].copy_from_slice(chunk);
        hash ^= u64::from_le_bytes(lane);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("blob-{hash:016x}-{}", bytes.len())
}
