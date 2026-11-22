use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use skia_safe::{Rect, TextBlob};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sprite {
    pub dest: Rect,
    pub src: Rect,
}

impl Sprite {
    pub fn new(src: Rect, dest: Rect) -> Self {
        Self { dest, src }
    }

    pub fn from_size(src: Rect, x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            dest: Rect::from_xywh(x, y, width, height),
            src,
        }
    }

    pub fn scale(&self) -> (f32, f32) {
        let src_width = self.src.width().max(1.0);
        let src_height = self.src.height().max(1.0);
        (
            self.dest.width() / src_width,
            self.dest.height() / src_height,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedText {
    pub text: String,
    pub x: f32,
    pub y: f32,
}

impl PlacedText {
    pub fn new(text: impl Into<String>, x: f32, y: f32) -> Self {
        Self {
            text: text.into(),
            x,
            y,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextBlobKey {
    family: Arc<str>,
    size: u32,
    bold: bool,
    text: Arc<str>,
}

impl TextBlobKey {
    pub fn new(family: &str, size: u32, bold: bool, text: &str) -> Self {
        Self {
            family: Arc::from(family),
            size,
            bold,
            text: Arc::from(text),
        }
    }

    pub fn family(&self) -> &str {
        &self.family
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn bold(&self) -> bool {
        self.bold
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextBlobStats {
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
    pub evicted: u64,
    pub shaped: u64,
}

struct BlobState {
    entries: HashMap<TextBlobKey, TextBlob>,
    order: VecDeque<TextBlobKey>,
}

impl BlobState {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn touch(&mut self, key: &TextBlobKey) {
        self.order.retain(|tracked| tracked != key);
        self.order.push_back(key.clone());
    }
}

pub struct TextBlobCache {
    capacity: usize,
    state: Mutex<BlobState>,
    hits: AtomicU64,
    misses: AtomicU64,
    evicted: AtomicU64,
    shaped: AtomicU64,
}

impl TextBlobCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(BlobState::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evicted: AtomicU64::new(0),
            shaped: AtomicU64::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.state.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, key: &TextBlobKey) -> Option<TextBlob> {
        let mut state = self.state.lock();
        let found = state.entries.get(key).cloned();
        match found {
            Some(blob) => {
                state.touch(key);
                self.hits.fetch_add(1, Ordering::SeqCst);
                Some(blob)
            }
            None => {
                self.misses.fetch_add(1, Ordering::SeqCst);
                None
            }
        }
    }

    pub fn insert(&self, key: TextBlobKey, blob: TextBlob) -> TextBlob {
        let mut state = self.state.lock();
        state.entries.insert(key.clone(), blob.clone());
        state.touch(&key);
        self.shaped.fetch_add(1, Ordering::SeqCst);
        while state.entries.len() > self.capacity {
            let Some(oldest) = state.order.pop_front() else {
                break;
            };
            state.entries.remove(&oldest);
            self.evicted.fetch_add(1, Ordering::SeqCst);
        }
        blob
    }

    pub fn clear(&self) {
        let mut state = self.state.lock();
        state.entries.clear();
        state.order.clear();
    }

    pub fn stats(&self) -> TextBlobStats {
        TextBlobStats {
            entries: self.len(),
            hits: self.hits.load(Ordering::SeqCst),
            misses: self.misses.load(Ordering::SeqCst),
            evicted: self.evicted.load(Ordering::SeqCst),
            shaped: self.shaped.load(Ordering::SeqCst),
        }
    }
}

impl std::fmt::Debug for TextBlobCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextBlobCache")
            .field("capacity", &self.capacity)
            .field("entries", &self.len())
            .finish()
    }
}
