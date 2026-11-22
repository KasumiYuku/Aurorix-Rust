use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::error::AssetError;
use crate::http::{BoxFuture, HttpClient, HttpRequest, HttpResponse};
use crate::provider::{MediaKinds, Provider, UploadInput};

pub struct MockHttp {
    requests: Mutex<Vec<HttpRequest>>,
    responses: Mutex<VecDeque<HttpResponse>>,
}

impl MockHttp {
    pub fn new(responses: Vec<HttpResponse>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into()),
        }
    }

    pub fn shared(responses: Vec<HttpResponse>) -> Arc<Self> {
        Arc::new(Self::new(responses))
    }

    pub fn count(&self) -> usize {
        self.requests.lock().len()
    }

    pub fn request(&self, index: usize) -> HttpRequest {
        self.requests.lock()[index].clone()
    }

    pub fn last(&self) -> HttpRequest {
        self.requests.lock().last().cloned().expect("应当有请求")
    }
}

impl HttpClient for MockHttp {
    fn execute<'a>(
        &'a self,
        request: HttpRequest,
    ) -> BoxFuture<'a, Result<HttpResponse, AssetError>> {
        Box::pin(async move {
            self.requests.lock().push(request);
            let response = self.responses.lock().pop_front();
            response.ok_or_else(|| AssetError::message("MockHttp: 没有预置响应"))
        })
    }
}

pub struct TestProvider {
    name: String,
    kinds: MediaKinds,
    limit: u64,
    session_scoped: bool,
    skip_probe: bool,
    outcomes: Mutex<VecDeque<Result<String, String>>>,
    calls: AtomicU32,
}

impl TestProvider {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            kinds: MediaKinds::IMAGE,
            limit: 0,
            session_scoped: false,
            skip_probe: true,
            outcomes: Mutex::new(VecDeque::new()),
            calls: AtomicU32::new(0),
        }
    }

    pub fn failing(name: &str, message: &str) -> Self {
        let provider = Self::new(name);
        provider.outcomes.lock().push_back(Err(message.to_string()));
        provider
    }

    pub fn always_failing(name: &str, message: &str) -> Self {
        let provider = Self::new(name);
        let mut outcomes = provider.outcomes.lock();
        for _ in 0..64 {
            outcomes.push_back(Err(message.to_string()));
        }
        drop(outcomes);
        provider
    }

    pub fn failing_times(name: &str, message: &str, times: usize) -> Self {
        let provider = Self::new(name);
        let mut outcomes = provider.outcomes.lock();
        for _ in 0..times {
            outcomes.push_back(Err(message.to_string()));
        }
        drop(outcomes);
        provider
    }

    pub fn with_kinds(mut self, kinds: MediaKinds) -> Self {
        self.kinds = kinds;
        self
    }

    pub fn with_max_size(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_session_scoped(mut self) -> Self {
        self.session_scoped = true;
        self
    }

    pub fn with_probe(mut self) -> Self {
        self.skip_probe = false;
        self
    }

    pub fn calls(&self) -> u32 {
        self.calls.load(Ordering::Relaxed)
    }
}

impl Provider for TestProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn kinds(&self) -> MediaKinds {
        self.kinds
    }

    fn max_size(&self) -> u64 {
        self.limit
    }

    fn session_scoped(&self) -> bool {
        self.session_scoped
    }

    fn skip_probe(&self) -> bool {
        self.skip_probe
    }

    fn upload<'a>(&'a self, _input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let outcome = self.outcomes.lock().pop_front();
            match outcome {
                Some(Ok(url)) => Ok(url),
                Some(Err(message)) => Err(AssetError::message(message)),
                None => Ok(format!("https://cdn.example/{}.png", self.name)),
            }
        })
    }
}

pub trait ResultExt<T> {
    fn failure(self) -> AssetError;
}

impl<T> ResultExt<T> for Result<T, AssetError> {
    fn failure(self) -> AssetError {
        match self {
            Ok(_) => panic!("应当失败"),
            Err(err) => err,
        }
    }
}

pub async fn official_lock() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(())).lock().await
}

pub fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut data = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    data.extend_from_slice(&[0, 0, 0, 0x0D]);
    data.extend_from_slice(b"IHDR");
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[0x08, 0x06, 0x00, 0x00, 0x00]);
    data
}

pub struct FakeClock {
    base: Instant,
    offset: Mutex<Duration>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            offset: Mutex::new(Duration::ZERO),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn advance(&self, delta: Duration) {
        let mut offset = self.offset.lock();
        *offset += delta;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::breaker::BreakerClock for FakeClock {
    fn now(&self) -> Instant {
        self.base + *self.offset.lock()
    }
}
