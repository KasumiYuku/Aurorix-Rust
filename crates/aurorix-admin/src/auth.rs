use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

pub const DEFAULT_TOKEN_TTL_SECONDS: u64 = 24 * 60 * 60;

pub const TOKEN_BYTES: usize = 32;

pub const SESSION_COOKIE: &str = "px_admin";

#[derive(Debug)]
pub struct SessionStore {
    ttl: Duration,
    counter: AtomicU64,
    sessions: Mutex<HashMap<String, Instant>>,
    capacity: usize,
}

impl SessionStore {
    pub fn new(ttl: Duration) -> Self {
        let ttl = if ttl.is_zero() {
            Duration::from_secs(DEFAULT_TOKEN_TTL_SECONDS)
        } else {
            ttl
        };
        Self {
            ttl,
            counter: AtomicU64::new(0),
            sessions: Mutex::new(HashMap::new()),
            capacity: aurorix_core::HardwareProfile::cached()
                .session_capacity()
                .max(4),
        }
    }

    pub fn with_ttl_seconds(seconds: u64) -> Self {
        Self::new(Duration::from_secs(seconds))
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn ttl_seconds(&self) -> u64 {
        self.ttl.as_secs()
    }

    pub fn issue(&self) -> (String, u64) {
        self.issue_for(self.ttl)
    }

    pub fn issue_for(&self, ttl: Duration) -> (String, u64) {
        let ttl = if ttl.is_zero() { self.ttl } else { ttl };
        let token = self.new_token();
        let mut sessions = self.sessions.lock().unwrap_or_else(|err| err.into_inner());
        let now = Instant::now();
        sessions.retain(|_, expire_at| *expire_at > now);
        while sessions.len() >= self.capacity {
            let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, expire_at)| **expire_at)
                .map(|(token, _)| token.clone())
            else {
                break;
            };
            sessions.remove(&oldest);
        }
        sessions.insert(token.clone(), now + ttl);
        (token, ttl.as_secs())
    }

    pub fn validate(&self, token: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap_or_else(|err| err.into_inner());
        let Some(expire_at) = sessions.get(token).copied() else {
            return false;
        };
        if Instant::now() >= expire_at {
            sessions.remove(token);
            return false;
        }
        true
    }

    pub fn revoke(&self, token: &str) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|err| err.into_inner());
        sessions.remove(token);
    }

    pub fn len(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn new_token(&self) -> String {
        let sequence = self.counter.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let mut buffer = [0u8; TOKEN_BYTES];
        let filled = std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut buffer))
            .is_ok();
        if !filled {
            let state = std::collections::hash_map::RandomState::new();
            for (index, slot) in buffer.chunks_mut(8).enumerate() {
                let mut hasher = state.build_hasher();
                hasher.write_u128(stamp);
                hasher.write_u64(sequence);
                hasher.write_usize(index);
                slot.copy_from_slice(&hasher.finish().to_le_bytes());
            }
        }
        URL_SAFE_NO_PAD.encode(buffer)
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::with_ttl_seconds(DEFAULT_TOKEN_TTL_SECONDS)
    }
}

pub fn password_matches(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();
    let mut difference = expected.len() ^ provided.len();
    let span = expected.len().max(provided.len());
    for index in 0..span {
        let left = expected.get(index).copied().unwrap_or(0);
        let right = provided.get(index).copied().unwrap_or(0);
        difference |= usize::from(left ^ right);
    }
    difference == 0
}

pub fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

pub fn cookie_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let mut found = None;
    for header in headers.get_all(axum::http::header::COOKIE) {
        let Ok(raw) = header.to_str() else {
            continue;
        };
        for part in raw.split(';') {
            if let Some(value) = part
                .trim()
                .strip_prefix(SESSION_COOKIE)
                .and_then(|rest| rest.strip_prefix('='))
            {
                if !value.is_empty() {
                    found = Some(value.to_string());
                }
            }
        }
    }
    found
}

pub fn session_token(headers: &axum::http::HeaderMap) -> Option<String> {
    bearer_token(headers).or_else(|| cookie_token(headers))
}
