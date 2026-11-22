pub mod args;
pub mod engine;
pub mod error;
pub mod html;
pub mod inline;
pub mod overrides;
pub mod qqcard;
pub mod registry;
pub mod url_codec;

pub use args::{arg_map, flatten, format_float, Arg, Args, FlatArgs};
pub use engine::{fill, process_each, process_template, ProcessedTemplate};
pub use error::{TemplateError, TemplateResult};
pub use html::escape_html;
pub use inline::{
    build_inline_command, inline_truthy, is_inline_call, render_inline_command, INLINE_TEXT_LIMIT,
};
pub use overrides::{
    Overrides, ReloadHandle, DEFAULT_OVERRIDE_DIR, DEFAULT_RELOAD_SECONDS, GLOBAL_NAMESPACE,
};
pub use qqcard::{card_styles, is_qqcard_call, render_qq_card};
pub use registry::{
    fill_for, fill_html_for, fill_html_template, fill_markdown_template, html_template_count,
    is_html_template_exist, is_markdown_template_exist, markdown_template_count, new_html_template,
    new_markdown_template, register_builtin_templates, Registry, Template, TemplateKind,
    BUILTIN_CARD_TEMPLATE,
};
pub use url_codec::{encode_value, query_escape, query_unescape};
