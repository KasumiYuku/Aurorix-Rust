pub mod builder;
pub mod callbacks;
pub mod error;
pub mod footer;
pub mod keyboard;

pub use builder::{auto_send, only, style, tip, visited, Builder, Opt};
pub use callbacks::CallbackRegistry;
pub use error::{ButtonError, ButtonResult};
pub use footer::{
    check_footer_placeholders, footer_action_of, footer_bucket_of, footer_button, footer_button_id,
    footer_button_rows, footer_buttons, footer_count_key, footer_dedup_of, footer_slot_of,
    footer_style, parse_footer, parse_footer_line, parse_footer_options, split_footer_line,
    FooterAction, FooterBucket, FooterClick, FooterClickError, FooterDedup, FooterHub, FooterKind,
    FooterParseResult, FooterScope, FooterSpec, FOOTER_DEFAULT_REPLY, FOOTER_ID_PREFIX,
    FOOTER_MAX_BUTTONS,
};
pub use keyboard::{
    normalize_font_size, qualify, set_command_normalizer, set_default_font_size, Action,
    AllowedPermission, Button, ButtonAction, ButtonRow, ButtonStyle, Keyboard, KeyboardStyle,
    Permission, RenderData, DEFAULT_UNSUPPORTED_TIP, FONT_SIZE_SMALL, MAX_BUTTONS_PER_ROW,
    MAX_ROWS,
};
