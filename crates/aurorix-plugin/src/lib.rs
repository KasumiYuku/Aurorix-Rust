pub mod access;
pub mod args;
pub mod assets;
pub mod ban;
pub mod card;
pub mod callback;
pub mod command;
pub mod config_field;
pub mod error;
pub mod help;
pub mod job;
pub mod rate;
pub mod registry;
pub mod reply;
pub mod roles;
pub mod router;

pub use aurorix_db::{PluginDb, PluginDbRegistry};

pub use access::{
    canonical_command_path, check_access, AccessBook, AccessConfig, AccessMode, AccessQuery,
    AccessRule, Deny, Maintenance, Verdict, DENY_MESSAGE_LIMIT,
};
pub use args::{
    parse_args, parse_tokens, tokenize, ArgError, ArgKind, ArgsSpec, FlagSpec, ParsedArgs,
    PositionalSpec,
};
pub use assets::{
    AssetPathError, PluginAssets, PluginAssetsReport, DEFAULT_MAX_ASSET_BYTES, FONTS_DIR,
    FONT_SUFFIXES, TEMPLATES_DIR, TEMPLATE_SUFFIX,
};
pub use callback::{CallbackBook, CallbackFuture, CallbackHandler, CallbackRequest};
pub use ban::{BanEntry, BanList, BAN_MESSAGE_LIMIT, BAN_THROTTLE, DEFAULT_BAN_MESSAGE};
pub use card::{argument_error, handler_failure, render_card, ARGUMENT_ERROR_TITLE};
pub use command::{
    async_handler, build_node, normalize_name, sync_handler, CommandHandler, CommandNode,
    CommandSpec, HandlerFuture, PluginCommands, PREFIX_SYMBOLS,
};
pub use config_field::{
    defaults, fields_json, find_field, normalize_values, resumed_values, validate_values,
    PluginConfigField, PluginConfigKind,
};
pub use error::{PluginError, PluginResult};
pub use help::{help_spec, render_help, render_plugin_help, HELP_HEADER};
pub use job::{
    async_job, sync_job, CronExpr, JobError, JobFuture, JobHandler, JobKind, JobRegistry,
    JobReport, JobRun, JobSchedule, JobSpec, JOB_MODULE,
};
pub use rate::{Rate, RateKey, RateLimiter};
pub use registry::{CommandRegistry, PluginSlot, RegistryIndex, RootMatch};
pub use reply::{
    AtPart, Attachment, ImageSource, Incoming, IncomingAttachment, Invocation, InvocationSender,
    Media, Outgoing, Painter, Part, PushSink, RenderCard, ReplyKind, ReplySink, SendFuture,
    MEDIA_FILE,
    MEDIA_IMAGE, MEDIA_VIDEO, MEDIA_VOICE,
};
pub use roles::{
    shared, FixedRoleResolver, PlatformRoleResolver, RoleResolver, StaticRoleResolver,
};
pub use router::{
    role_allows, split_fields, usage_text, DispatchOutcome, PluginAssetBook, Router, Stage,
};

pub use aurorix_core::{RoleRequired, Scene, Target};
