use aurorix_protocol::{EventKind, GatewayEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HEARTBEAT_CEILING_MS: u64 = 45_000;
pub const TOKEN_PREFIX: &str = "QQBot ";

pub const OP_DISPATCH: i32 = 0;
pub const OP_HEARTBEAT: i32 = 1;
pub const OP_IDENTIFY: i32 = 2;
pub const OP_RESUME: i32 = 6;
pub const OP_RECONNECT: i32 = 7;
pub const OP_INVALID_SESSION: i32 = 9;
pub const OP_HELLO: i32 = 10;
pub const OP_HEARTBEAT_ACK: i32 = 11;
pub const OP_CALLBACK_VALIDATE: i32 = 13;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Properties {
    #[serde(rename = "$os")]
    pub os: String,
    #[serde(rename = "$browser")]
    pub browser: String,
    #[serde(rename = "$device")]
    pub device: String,
}

impl Default for Properties {
    fn default() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            browser: String::new(),
            device: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdentifyPayload {
    pub token: String,
    pub intents: u64,
    pub shard: [u32; 2],
    pub properties: Properties,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResumePayload {
    pub token: String,
    pub session_id: String,
    pub seq: i64,
    pub shard: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct HeartbeatPayload {
    op: i32,
    d: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FrameBody<T> {
    op: i32,
    d: T,
}

pub fn auth_token(token: &str) -> String {
    let mut value = String::from(TOKEN_PREFIX);
    value.push_str(token);
    value
}

pub fn identify_frame(token: &str, intents: u64, shard: [u32; 2]) -> String {
    let body = FrameBody {
        op: OP_IDENTIFY,
        d: IdentifyPayload {
            token: auth_token(token),
            intents,
            shard,
            properties: Properties::default(),
        },
    };
    serde_json::to_string(&body).unwrap_or_default()
}

pub fn resume_frame(token: &str, session_id: &str, seq: i64, shard: [u32; 2]) -> String {
    let body = FrameBody {
        op: OP_RESUME,
        d: ResumePayload {
            token: auth_token(token),
            session_id: session_id.to_string(),
            seq,
            shard,
        },
    };
    serde_json::to_string(&body).unwrap_or_default()
}

pub fn heartbeat_frame(seq: i64) -> String {
    let body = HeartbeatPayload {
        op: OP_HEARTBEAT,
        d: seq,
    };
    serde_json::to_string(&body).unwrap_or_default()
}

pub fn heartbeat_interval_ms(raw: u64) -> u64 {
    if raw > HEARTBEAT_CEILING_MS {
        HEARTBEAT_CEILING_MS
    } else {
        raw
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolEvent {
    pub id: Option<String>,
    pub op: i32,
    pub event_type: Option<String>,
    pub seq: Option<i64>,
    pub data: Value,
    pub raw: String,
}

impl ProtocolEvent {
    pub fn kind(&self) -> Option<EventKind> {
        self.event_type.as_deref().and_then(EventKind::parse)
    }

    pub fn is_known(&self) -> bool {
        self.kind().is_some()
    }

    pub fn is_lifecycle(&self) -> bool {
        matches!(self.event_type.as_deref(), Some("READY") | Some("RESUMED"))
    }

    pub fn data_text(&self) -> String {
        self.data.to_string()
    }

    pub fn from_text(text: &str) -> Option<ProtocolEvent> {
        if let Ok(frame) = serde_json::from_str::<GatewayEvent>(text) {
            return Some(ProtocolEvent {
                id: frame.id,
                op: frame.op,
                event_type: frame.t,
                seq: frame.s,
                data: frame.d,
                raw: text.to_string(),
            });
        }
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(_) => return None,
        };
        let op = match value.get("op").and_then(Value::as_i64) {
            Some(code) => code as i32,
            None => -1,
        };
        Some(ProtocolEvent {
            id: value.get("id").and_then(Value::as_str).map(str::to_string),
            op,
            event_type: value.get("t").and_then(Value::as_str).map(str::to_string),
            seq: value.get("s").and_then(Value::as_i64),
            data: value.get("d").cloned().unwrap_or(Value::Null),
            raw: text.to_string(),
        })
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<ProtocolEvent> {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(ProtocolEvent::from_text)
    }
}

pub fn parse_hello(text: &str) -> Option<u64> {
    let frame: Value = serde_json::from_str(text).ok()?;
    let op = frame.get("op").and_then(Value::as_i64)?;
    if op != i64::from(OP_HELLO) {
        return None;
    }
    let interval = frame
        .get("d")
        .and_then(|data| data.get("heartbeat_interval"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(heartbeat_interval_ms(interval))
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReadyInfo {
    pub session_id: String,
    pub bot_id: String,
    pub bot_name: String,
    pub shard: Option<u32>,
}

pub fn parse_ready(text: &str) -> ReadyInfo {
    let mut info = ReadyInfo::default();
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(_) => return info,
    };
    let data = match value.get("d") {
        Some(data) => data,
        None => return info,
    };
    if let Some(session) = data.get("session_id").and_then(Value::as_str) {
        info.session_id = session.to_string();
    }
    if let Some(user) = data.get("user") {
        if let Some(id) = user.get("id").and_then(Value::as_str) {
            info.bot_id = id.to_string();
        }
        if let Some(name) = user.get("username").and_then(Value::as_str) {
            info.bot_name = name.to_string();
        }
    }
    if let Some(shard) = data.get("shard").and_then(Value::as_u64) {
        info.shard = Some(shard as u32);
    }
    info
}

pub fn parse_resumed_session(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    value
        .get("d")
        .and_then(|data| data.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}
