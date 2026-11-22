pub mod error;
pub mod event;
pub mod intents;
pub mod message;
pub mod op;
pub mod retry;

pub use error::{ProtocolError, ProtocolResult};
pub use event::{EventKind, GatewayEvent};
pub use intents::{IntentSet, DEFAULT_INTENTS};
pub use message::{
    MessageKind, SendRequest, APP_ACCESS_TOKEN_PATH, CHANNEL_MESSAGE_PATH, DM_MESSAGE_PATH,
    GROUP_MESSAGE_PATH, USER_MESSAGE_PATH,
};
pub use op::WebSocketOp;
pub use retry::{should_retry, RetryPolicy, DEFAULT_RETRY_WHEN};

pub use aurorix_core::{Scene, Target};
