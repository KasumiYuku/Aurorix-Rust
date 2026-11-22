pub mod backoff;
pub mod client;
pub mod codes;
pub mod error;
pub mod frame;
pub mod intents;
pub mod sink;
pub mod source;
pub mod state;
pub mod transport;
pub mod webhook;

pub use backoff::{double, jitter, Backoff, BASE_DELAY, MAX_DELAY};
pub use client::{GatewayClient, GatewayConfig, DEFAULT_GATEWAY_URL};
pub use codes::{
    can_resume, classify, is_permanent, need_reidentify, CloseAction, AUTH_FAIL, BOT_BANNED,
    BOT_OFFLINE, INVALID_SEQ, SESSION_GONE,
};
pub use error::GatewayError;
pub use frame::{
    auth_token, heartbeat_frame, heartbeat_interval_ms, identify_frame, parse_hello, parse_ready,
    parse_resumed_session, resume_frame, ProtocolEvent, ReadyInfo, HEARTBEAT_CEILING_MS,
    OP_CALLBACK_VALIDATE, OP_DISPATCH, OP_HEARTBEAT, OP_HEARTBEAT_ACK, OP_HELLO, OP_IDENTIFY,
    OP_INVALID_SESSION, OP_RECONNECT, OP_RESUME, TOKEN_PREFIX,
};
pub use intents::{
    bits_for_names, bits_for_set, ensure_protocol, intent_bit, requires_websocket, unknown_names,
    Protocol, DEFAULT_INTENT_BITS, EVENT_INTENTS, GUILD_INTENT_BITS, INTENT_EVENTS,
};
pub use sink::BroadcastSink;
pub use source::{
    fixed_headers, BoxFuture, EventSink, GatewayEndpoint, GatewaySource, StaticTokenSource,
    TokenSource,
};
pub use state::{GatewayState, GatewayStats};
pub use transport::{ConnectRequest, Frame, FrameIo, Transport, WebSocketTransport};
pub use webhook::{
    callback_signature, ensure_webhook_compatible, raw_timestamp, signing_key, signing_seed,
    verify_signature, verifying_key, webhook_router, WebhookConfig, WebhookOutcome,
    WebhookProcessor, WebhookReply, SIGNATURE_HEADER, TIMESTAMP_HEADER, WEBHOOK_PATH,
};
