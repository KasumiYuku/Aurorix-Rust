pub mod bot;
pub mod card_cache;
pub mod config;
pub mod error;
pub mod event;
pub mod gateway_source;
pub mod images;
pub mod interaction;
pub mod jobs;
pub mod lock;
pub mod media;
pub mod notice;
pub mod panels;
pub mod pipeline;
pub mod plugin_config;
pub mod pool;
pub mod ports;
pub mod webhook;

pub use bot::{
    wait_for_shutdown_signal, Bot, BotBuilder, ControlCommand, FallbackPolicy, Runtime,
    RuntimeHandle, ShutdownReport, DEFAULT_EVENT_BUFFER, DEFAULT_SHUTDOWN_TIMEOUT, MODULE,
    SUPERVISED_ENV, UNSUPERVISED_RESTART_ERROR,
};
pub use config::{
    BotConfig, DEFAULT_ADMIN_LISTEN, DEFAULT_ADMIN_PORT, DEFAULT_LOG_LEVEL, DEFAULT_PROTOCOL,
    DEFAULT_PROXY_API, DEFAULT_RENDER_POOL, LOG_LEVELS,
};
pub use error::{BotError, FALLBACK_HINT};
pub use event::{mention_tag, EventCategory, MessageEvent, MESSAGE_EVENTS};
pub use gateway_source::{
    OfficialGatewaySource, GATEWAY_INFO_FAILURE_DELAY, GATEWAY_SESSION_DELAY,
};
pub use lock::{
    holder_pid, process_alive, InstanceLock, DEFAULT_LOCK_FILE, INSTANCE_CONFLICT, LOCK_FILE_ENV,
};
pub use notice::{avatar_url, compose, mention, pick_index, pick_line, NoticeEvent, NoticeKind};
pub use panels::ApiPanelRemote;
pub use pipeline::{
    classify_send_error, Outcome, Pipeline, PipelineCounters, PipelineDeps, ProcessReport,
    PushReport, CARD_REPLY_KINDS,
};
pub use plugin_config::PluginConfigStore;
pub use pool::{
    limits, limits_for, WorkPool, POOL_MAX, POOL_QUEUE, POOL_TIMEOUT,
};
pub use ports::{
    ApiTokens, CardView, ClosurePluginConfig, DbStats, InboxArchive, InboxBatch, InboxMessage,
    KvBackend, MemoryInbox, MemoryKv, MemoryStats, PluginConfigurable, RejectedTokens, Rendered,
    Renderer, ReplyTransport, SchedulerRenderer, StatsDelta, StatsRecorder,
};

pub use aurorix_core::{RoleRequired, Scene, Target};
pub use aurorix_gateway::{
    EventSink, Frame, FrameIo, GatewayClient, GatewayState, ProtocolEvent, TokenSource, Transport,
    WebSocketTransport,
};
pub use aurorix_log::{Level, Logger, Options};
pub use aurorix_plugin::{
    AccessBook, CommandRegistry, CommandSpec, Incoming, Outgoing, ReplyKind, ReplySink, Router,
    Stage,
};
pub use aurorix_render::{RenderError, RenderScheduler, SchedulerConfig};
pub use images::{build_host, current_target, with_target, HostImageStore, HostReload};
pub use interaction::{InteractionEvent, INTERACTION_TYPE_BUTTON};
pub use webhook::{ensure_webhook_intents, webhook_events, WebhookIngress, WEBHOOK_MODE_LABEL};
