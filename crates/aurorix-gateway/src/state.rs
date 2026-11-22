use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GatewayState {
    #[default]
    Disconnected,
    Connecting,
    Identified,
    Resuming,
    Ready,
    Stopped,
}

impl GatewayState {
    pub fn as_str(self) -> &'static str {
        match self {
            GatewayState::Disconnected => "disconnected",
            GatewayState::Connecting => "connecting",
            GatewayState::Identified => "identified",
            GatewayState::Resuming => "resuming",
            GatewayState::Ready => "ready",
            GatewayState::Stopped => "stopped",
        }
    }

    pub fn is_ready(self) -> bool {
        self == GatewayState::Ready
    }

    pub fn is_terminal(self) -> bool {
        self == GatewayState::Stopped
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayStats {
    pub state: GatewayState,
    pub reconnects: u64,
    pub last_close_code: Option<u16>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub seq: i64,
    pub ready: bool,
    pub session_id: Option<String>,
    pub bot_id: Option<String>,
    pub bot_name: Option<String>,
    pub shard: [u32; 2],
    pub heartbeat_interval_ms: Option<u64>,
    pub connected_since_ms: i64,
    pub live_tasks: usize,
}

impl Default for GatewayStats {
    fn default() -> Self {
        Self {
            state: GatewayState::Disconnected,
            reconnects: 0,
            last_close_code: None,
            last_event_at: None,
            seq: 0,
            ready: false,
            session_id: None,
            bot_id: None,
            bot_name: None,
            shard: [0, 1],
            heartbeat_interval_ms: None,
            connected_since_ms: 0,
            live_tasks: 0,
        }
    }
}
