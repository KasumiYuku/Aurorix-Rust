pub mod api;
pub mod audit;
pub mod dedup;
pub mod error;
pub mod interaction;
pub mod image_box;
pub mod markdown;
pub mod message;
pub mod panels;
pub mod ratelimit;
pub mod recall;
pub mod text;
pub mod token;
pub mod transport;
pub mod upload;

pub use api::{
    delivery_steps, passive_gone, BotApi, BotConfig, GatewayBotInfo, GroupInfo, SessionStartLimit,
    DEFAULT_BASE_URL,
};
pub use audit::{AuditOutcome, AuditRegistry, AUDIT_PENDING_CODE, AUDIT_TIMEOUT};
pub use dedup::{DedupWindow, DEFAULT_DEDUP_TTL};
pub use error::{classify, describe, is_retryable, ApiError, BusinessError, ErrorKind};
pub use image_box::{
    clamp_markdown, ImageBox, DEFAULT_IMAGE_MAX_HEIGHT, DEFAULT_IMAGE_MAX_WIDTH, MAX_IMAGE_SIDE,
};
pub use interaction::{interaction_path, INTERACTION_ACK_CODE, INTERACTION_ACK_PATH};
pub use markdown::{
    cmd_escape, encode_cmd_tags, fit_images, prepare, prepare_with, protect_at, BoxFuture,
    ImageStore, Prepared, PreparedImage, StoredImage, CHANNEL_TEXT_LIMIT, ZERO_WIDTH_SPACE,
};
pub use message::{
    Anchor, DeliveryStep, Outbound, SendOutcome, SendResult, SeqBudget, MAX_PASSIVE_SEQ,
};
pub use panels::{
    panel_display_width, valid_panel_scope, validate_panel, Panel, PanelItem, PanelPage,
    PanelRecord, PANEL_DESC_MAX_WIDTH, PANEL_ITEM_COMMAND, PANEL_ITEM_LINK, PANEL_MAX_ITEMS,
    PANEL_MAX_PAGES, PANEL_MAX_PER_BOT, PANEL_NAME_MAX_WIDTH, PANEL_PAGE_LIMIT,
    PANEL_REMARK_MAX_LEN, PANEL_TARGET_ALL,
};
pub use ratelimit::{RateLimiter, PUSH_KEY, PUSH_RATE_LIMIT, PUSH_RATE_WINDOW};
pub use recall::{RecallFailure, RecallReport, SentBatch, RECALL_GRACE, RECALL_WINDOW};
pub use text::{guild_text, split_images, strip_latex, truncate_runes, GUILD_TEXT_LIMIT};
pub use token::{TokenManager, DEFAULT_TOKEN_URL, EXPIRY_MARGIN, FAILURE_COOLDOWN};
pub use transport::{
    EVENT_ID_RETRY_CODE, EVENT_ID_RETRY_DELAY, EVENT_ID_RETRY_LIMIT, RETRY_BASE_DELAY, RETRY_LIMIT,
};
pub use upload::{
    md5_10m_hex, md5_hex, media_base, MediaResult, MediaUpload, DEFAULT_UPLOAD_THRESHOLD,
    MD5_HEAD_SIZE, MEDIA_FILE, MEDIA_IMAGE, MEDIA_VIDEO, MEDIA_VOICE, PART_ATTEMPTS,
    PART_RETRY_DELAY,
};

pub use aurorix_core::{Scene, Target};
pub use aurorix_protocol::{MessageKind, SendRequest};
