use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::error::GatewayError;
use crate::frame::ProtocolEvent;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait TokenSource: Send + Sync {
    fn token(&self) -> BoxFuture<'_, Result<String, GatewayError>>;
    fn invalidate(&self) -> BoxFuture<'_, ()>;
}

pub trait EventSink: Send + Sync {
    fn dispatch(&self, event: ProtocolEvent);
}

#[derive(Debug)]
pub struct StaticTokenSource {
    token: Mutex<String>,
    invalidations: AtomicU64,
}

impl StaticTokenSource {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: Mutex::new(token.into()),
            invalidations: AtomicU64::new(0),
        }
    }

    pub fn invalidations(&self) -> u64 {
        self.invalidations.load(Ordering::SeqCst)
    }

    pub fn set(&self, token: impl Into<String>) {
        if let Ok(mut guard) = self.token.lock() {
            *guard = token.into();
        }
    }
}

impl TokenSource for StaticTokenSource {
    fn token(&self) -> BoxFuture<'_, Result<String, GatewayError>> {
        let value = match self.token.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => String::new(),
        };
        Box::pin(async move {
            if value.is_empty() {
                return Err(GatewayError::Token("access token 为空".to_string()));
            }
            Ok(value)
        })
    }

    fn invalidate(&self) -> BoxFuture<'_, ()> {
        self.invalidations.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {})
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayEndpoint {
    pub url: String,
    pub shards: u32,
}

impl GatewayEndpoint {
    pub fn new(url: impl Into<String>, shards: u32) -> Self {
        Self {
            url: url.into(),
            shards: shards.max(1),
        }
    }

    pub fn shard(&self) -> [u32; 2] {
        [0, self.shards.max(1)]
    }
}

pub trait GatewaySource: Send + Sync {
    fn endpoint(&self) -> BoxFuture<'_, Result<GatewayEndpoint, GatewayError>>;
    fn reauth_delay(&self) -> BoxFuture<'_, Duration>;
}

pub fn fixed_headers(app_id: &str) -> Vec<(String, String)> {
    vec![(String::from("X-Bot-Appid"), app_id.to_string())]
}
