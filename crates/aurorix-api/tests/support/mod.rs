#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aurorix_api::{BotApi, BotConfig};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::Router;

pub const TOKEN_PATH: &str = "/app/getAppAccessToken";
pub const ACCESS_TOKEN: &str = "tok-1";

#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub at: Instant,
}

impl Recorded {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }

    pub fn header(&self, name: &str) -> String {
        self.headers
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    pub fn url(&self) -> String {
        match &self.query {
            Some(query) => format!("{}?{}", self.path, query),
            None => self.path.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: u16,
    pub body: String,
}

impl MockResponse {
    pub fn json(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
        }
    }

    pub fn status(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }

    pub fn empty() -> Self {
        Self {
            status: 200,
            body: String::new(),
        }
    }

    pub fn ok_id(id: &str) -> Self {
        Self::json(format!(r#"{{"id":"{id}"}}"#))
    }

    pub fn platform(code: i32, message: &str) -> Self {
        Self::status(400, format!(r#"{{"code":{code},"message":"{message}"}}"#))
    }
}

pub fn token_ok() -> MockResponse {
    MockResponse::json(format!(
        r#"{{"access_token":"{ACCESS_TOKEN}","expires_in":"7200"}}"#
    ))
}

pub struct Scripted {
    queue: Mutex<VecDeque<MockResponse>>,
    fallback: MockResponse,
    token: Mutex<Option<MockResponse>>,
}

impl Scripted {
    pub fn new(responses: Vec<MockResponse>) -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(responses.into()),
            fallback: MockResponse::ok_id("fallback"),
            token: Mutex::new(None),
        })
    }

    pub fn with_fallback(responses: Vec<MockResponse>, fallback: MockResponse) -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(responses.into()),
            fallback,
            token: Mutex::new(None),
        })
    }

    pub fn with_token(responses: Vec<MockResponse>, token: MockResponse) -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(responses.into()),
            fallback: MockResponse::ok_id("fallback"),
            token: Mutex::new(Some(token)),
        })
    }

    pub fn handle(&self, record: &Recorded) -> MockResponse {
        if record.path.ends_with(TOKEN_PATH) {
            if let Ok(slot) = self.token.lock() {
                if let Some(response) = slot.as_ref() {
                    return response.clone();
                }
            }
            return token_ok();
        }
        let mut queue = self.queue.lock().expect("脚本队列不能中毒");
        queue.pop_front().unwrap_or_else(|| self.fallback.clone())
    }
}

pub type Responder = Arc<dyn Fn(&Recorded, usize) -> MockResponse + Send + Sync>;

pub struct MockState {
    pub requests: Mutex<Vec<Recorded>>,
    pub responder: Responder,
}

pub struct MockServer {
    pub base_url: String,
    pub state: Arc<MockState>,
}

impl MockServer {
    pub async fn with_script(responses: Vec<MockResponse>) -> Self {
        let scripted = Scripted::new(responses);
        Self::with_scripted(scripted).await
    }

    pub async fn with_scripted(scripted: Arc<Scripted>) -> Self {
        let shared = Arc::clone(&scripted);
        Self::start(move |record: &Recorded, _index: usize| shared.handle(record)).await
    }

    pub async fn start<F>(responder: F) -> Self
    where
        F: Fn(&Recorded, usize) -> MockResponse + Send + Sync + 'static,
    {
        let state = Arc::new(MockState {
            requests: Mutex::new(Vec::new()),
            responder: Arc::new(responder),
        });
        let app = Router::new()
            .fallback(handle)
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("监听随机端口");
        let addr = listener.local_addr().expect("取本地地址");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            base_url: format!("http://{addr}"),
            state,
        }
    }

    pub fn api(&self) -> BotApi {
        BotApi::new(
            BotConfig::new("app", "secret")
                .with_base_url(self.base_url.clone())
                .with_token_url(format!("{}{TOKEN_PATH}", self.base_url)),
        )
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.state.requests.lock().expect("请求表不能中毒").clone()
    }

    pub fn count(&self) -> usize {
        self.requests().len()
    }

    pub fn count_token(&self) -> usize {
        self.requests()
            .iter()
            .filter(|record| record.path.ends_with(TOKEN_PATH))
            .count()
    }

    pub fn api_requests(&self) -> Vec<Recorded> {
        self.requests()
            .into_iter()
            .filter(|record| !record.path.ends_with(TOKEN_PATH))
            .collect()
    }

    pub fn api_count(&self) -> usize {
        self.api_requests().len()
    }

    pub fn paths(&self) -> Vec<String> {
        self.api_requests()
            .into_iter()
            .map(|record| record.path)
            .collect()
    }

    pub fn last(&self) -> Recorded {
        self.api_requests().pop().expect("至少要有一个业务请求")
    }
}

async fn handle(
    State(state): State<Arc<MockState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let mut collected: HashMap<String, String> = HashMap::new();
    for (name, value) in headers.iter() {
        collected.insert(
            name.as_str().to_ascii_lowercase(),
            value.to_str().unwrap_or_default().to_string(),
        );
    }
    let record = Recorded {
        method: method.as_str().to_string(),
        path: uri.path().to_string(),
        query: uri.query().map(str::to_string),
        headers: collected,
        body: body.to_vec(),
        at: Instant::now(),
    };
    let index = {
        let mut requests = state.requests.lock().expect("请求表不能中毒");
        requests.push(record.clone());
        requests.len() - 1
    };
    let response = (state.responder)(&record, index);
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
    (status, response.body)
}
