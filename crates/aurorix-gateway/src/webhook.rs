use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde_json::{json, Value};

use crate::error::GatewayError;
use crate::frame::{ProtocolEvent, OP_CALLBACK_VALIDATE};
use crate::intents::{ensure_protocol, Protocol};
use crate::source::EventSink;

pub const WEBHOOK_PATH: &str = "/webhook";
pub const SIGNATURE_HEADER: &str = "X-Signature-Ed25519";
pub const TIMESTAMP_HEADER: &str = "X-Signature-Timestamp";
pub const SEED_LEN: usize = 32;
pub const SIGNATURE_LEN: usize = 64;
pub const SIGNATURE_HIGH_BITS: u8 = 224;

pub fn signing_seed(secret: &str) -> [u8; SEED_LEN] {
    let mut seed = [0u8; SEED_LEN];
    let bytes = secret.as_bytes();
    if bytes.is_empty() {
        return seed;
    }
    let mut filled = 0usize;
    while filled < SEED_LEN {
        for byte in bytes {
            if filled >= SEED_LEN {
                break;
            }
            seed[filled] = *byte;
            filled += 1;
        }
    }
    seed
}

pub fn signing_key(secret: &str) -> SigningKey {
    SigningKey::from_bytes(&signing_seed(secret))
}

pub fn verifying_key(secret: &str) -> VerifyingKey {
    signing_key(secret).verifying_key()
}

pub fn verify_signature(
    secret: &str,
    signature: Option<&str>,
    timestamp: Option<&str>,
    body: &[u8],
) -> Result<(), GatewayError> {
    let signature = match signature {
        Some(value) if !value.is_empty() => value,
        _ => {
            return Err(GatewayError::Config(
                "签名校验失败: 缺少 X-Signature-Ed25519".to_string(),
            ))
        }
    };
    let timestamp = match timestamp {
        Some(value) if !value.is_empty() => value,
        _ => {
            return Err(GatewayError::Config(
                "签名校验失败: 缺少 X-Signature-Timestamp".to_string(),
            ))
        }
    };
    let raw = hex::decode(signature)
        .map_err(|_| GatewayError::Config("签名校验失败: 签名不是合法 hex".to_string()))?;
    if raw.len() != SIGNATURE_LEN {
        return Err(GatewayError::Config(
            "签名校验失败: 签名长度不是 64 字节".to_string(),
        ));
    }
    if raw[SIGNATURE_LEN - 1] & SIGNATURE_HIGH_BITS != 0 {
        return Err(GatewayError::Config(
            "签名校验失败: 签名高位不为零".to_string(),
        ));
    }
    let mut bytes = [0u8; SIGNATURE_LEN];
    bytes.copy_from_slice(&raw);
    let signature = Signature::from_bytes(&bytes);
    let mut message = Vec::with_capacity(timestamp.len() + body.len());
    message.extend_from_slice(timestamp.as_bytes());
    message.extend_from_slice(body);
    verifying_key(secret)
        .verify_strict(&message, &signature)
        .map_err(|_| GatewayError::Config("签名校验失败: 验签不通过".to_string()))
}

pub fn raw_timestamp(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

pub fn callback_signature(secret: &str, event_ts: &str, plain_token: &str) -> String {
    let mut message = String::from(event_ts);
    message.push_str(plain_token);
    hex::encode(signing_key(secret).sign(message.as_bytes()).to_bytes())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookReply {
    pub status: u16,
    pub body: Option<Value>,
}

impl WebhookReply {
    pub fn accepted() -> Self {
        Self {
            status: 200,
            body: None,
        }
    }

    pub fn payload(body: Value) -> Self {
        Self {
            status: 200,
            body: Some(body),
        }
    }

    pub fn unauthorized() -> Self {
        Self {
            status: 401,
            body: None,
        }
    }

    pub fn bad_request() -> Self {
        Self {
            status: 400,
            body: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WebhookOutcome {
    pub reply: WebhookReply,
    pub event: Option<ProtocolEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookConfig {
    pub app_secret: String,
    pub subscriptions: Vec<String>,
    pub protocol: Protocol,
}

impl WebhookConfig {
    pub fn new(app_secret: impl Into<String>, subscriptions: Vec<String>) -> Self {
        Self {
            app_secret: app_secret.into(),
            subscriptions,
            protocol: Protocol::Webhook,
        }
    }

    pub fn validate(&self) -> Result<(), GatewayError> {
        ensure_protocol(self.protocol, &self.subscriptions)
    }
}

pub fn ensure_webhook_compatible(names: &[String]) -> Result<(), GatewayError> {
    ensure_protocol(Protocol::Webhook, names)
}

pub struct WebhookProcessor {
    secret: String,
    sink: Arc<dyn EventSink>,
}

impl WebhookProcessor {
    pub fn new(secret: impl Into<String>, sink: Arc<dyn EventSink>) -> Self {
        Self {
            secret: secret.into(),
            sink,
        }
    }

    pub fn secret(&self) -> &str {
        self.secret.as_str()
    }

    pub fn sink(&self) -> Arc<dyn EventSink> {
        Arc::clone(&self.sink)
    }

    pub fn handle(
        &self,
        signature: Option<&str>,
        timestamp: Option<&str>,
        body: &[u8],
    ) -> WebhookOutcome {
        if verify_signature(&self.secret, signature, timestamp, body).is_err() {
            return WebhookOutcome {
                reply: WebhookReply::unauthorized(),
                event: None,
            };
        }
        let payload: Value = match serde_json::from_slice(body) {
            Ok(value) => value,
            Err(_) => {
                return WebhookOutcome {
                    reply: WebhookReply::accepted(),
                    event: None,
                }
            }
        };
        let op = payload.get("op").and_then(Value::as_i64).unwrap_or(-1);
        if op == i64::from(OP_CALLBACK_VALIDATE) {
            let data = match payload.get("d") {
                Some(data) => data,
                None => {
                    return WebhookOutcome {
                        reply: WebhookReply::bad_request(),
                        event: None,
                    }
                }
            };
            let event_ts = data.get("event_ts").map(raw_timestamp).unwrap_or_default();
            let plain_token = data
                .get("plain_token")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let signature = callback_signature(&self.secret, &event_ts, plain_token);
            return WebhookOutcome {
                reply: WebhookReply::payload(json!({
                    "plain_token": plain_token,
                    "signature": signature,
                })),
                event: None,
            };
        }
        let event_type = payload.get("t").and_then(Value::as_str).unwrap_or_default();
        if event_type.is_empty() {
            return WebhookOutcome {
                reply: WebhookReply::accepted(),
                event: None,
            };
        }
        WebhookOutcome {
            reply: WebhookReply::accepted(),
            event: ProtocolEvent::from_bytes(body),
        }
    }
}

pub fn webhook_router(processor: Arc<WebhookProcessor>) -> Router {
    Router::new()
        .route(WEBHOOK_PATH, post(webhook_handler))
        .with_state(processor)
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn render(reply: WebhookReply) -> Response {
    let status = match StatusCode::from_u16(reply.status) {
        Ok(status) => status,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    match reply.body {
        Some(body) => (status, axum::Json(body)).into_response(),
        None => status.into_response(),
    }
}

async fn webhook_handler(
    State(processor): State<Arc<WebhookProcessor>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let signature = header_value(&headers, SIGNATURE_HEADER);
    let timestamp = header_value(&headers, TIMESTAMP_HEADER);
    let outcome = processor.handle(signature, timestamp, &body);
    if let Some(event) = outcome.event {
        let sink = processor.sink();
        tokio::spawn(async move {
            sink.dispatch(event);
        });
    }
    render(outcome.reply)
}
