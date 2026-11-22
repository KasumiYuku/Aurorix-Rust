# aurorix-buttons API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/builder.rs`

- `pub struct Opt(Box<dyn FnOnce(&mut Button) + Send>)`
- `impl Opt`
  - `pub fn apply(self, button: &mut Button)`
- `pub fn style(style: ButtonStyle) -> Opt`
- `pub fn visited(label: impl Into<String>) -> Opt`
- `pub fn only(users: Vec<String>) -> Opt`
- `pub fn auto_send() -> Opt`
- `pub fn tip(text: impl Into<String>) -> Opt`
- `pub struct Builder`
- `impl Builder`
  - `pub fn new() -> Self`
  - `pub fn scope(mut self, plugin_id: impl Into<String>) -> Self`
  - `pub fn row(mut self) -> Self`
  - `pub fn font_size(mut self, size: &str) -> Self`
  - `pub fn small_font(self) -> Self`
  - `pub fn link(self, id: &str, label: &str, url: &str, opts: Vec<Opt>) -> Self`
  - `pub fn callback(self, id: &str, label: &str, data: &str, opts: Vec<Opt>) -> Self`
  - `pub fn command(self, id: &str, label: &str, content: &str, opts: Vec<Opt>) -> Self`
  - `pub fn build(self) -> Option<Keyboard>`
  - `pub fn keyboard(&self) -> &Keyboard`
- `pub fn all_users() -> AllowedPermission`

## `src/callbacks.rs`

- `pub struct CallbackRegistry`
- `impl CallbackRegistry`
  - `pub fn new() -> Self`
  - `pub fn global() -> &'static CallbackRegistry`
  - `pub fn register(&self, id: &str, handler: Arc<dyn Any + Send + Sync>) -> bool`
  - `pub fn has(&self, id: &str) -> bool`
  - `pub fn get(&self, id: &str) -> Option<Arc<dyn Any + Send + Sync>>`
  - `pub fn ids(&self) -> Vec<String>`
  - `pub fn clear(&self)`

## `src/error.rs`

- `pub enum ButtonError`
- `pub type ButtonResult<T> = Result<T, ButtonError>`

## `src/footer.rs`

- `pub const FOOTER_ID_PREFIX: &str = "aurorix.footer."`
- `pub const FOOTER_MAX_BUTTONS: i32 = 5`
- `pub const FOOTER_OPTION_SEP: char = ';'`
- `pub const FOOTER_DEFAULT_REPLY: &str = "已记录，当前 {{count}} 次。"`
- `pub const FOOTER_REPLY_PLACEHOLDERS: [&str; 4] = ["count", "user", "group", "label"]`
- `pub enum FooterScope`
- `impl FooterScope`
  - `pub const ALL: [FooterScope; 7] = [`
  - `pub const fn slot_base(self) -> i32`
  - `pub const fn slug(self) -> &'static str`
  - `pub const fn label(self) -> &'static str`
  - `pub const fn from_scene(scene: Scene) -> Option<Self>`
- `pub fn footer_slot_of(slot: i32) -> Option<(FooterScope, i32)>`
- `pub enum FooterKind`
- `impl FooterKind`
  - `pub const fn as_str(self) -> &'static str`
  - `pub fn parse(raw: &str) -> Result<Self, String>`
  - `pub const fn allowed_options(self) -> &'static [&'static str]`
- `pub struct FooterSpec`
- `impl FooterSpec`
  - `pub fn option(&self, name: &str, fallback: &str) -> String`
  - `pub fn kind(&self) -> FooterKind`
- `pub struct FooterParseResult`
- `impl FooterParseResult`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn row_specs(&self, index: usize) -> Vec<&FooterSpec>`
- `pub enum FooterDedup`
- `pub enum FooterBucket`
- `pub enum FooterAction`
- `pub struct FooterClick`
- `pub enum FooterClickError`
- `pub fn footer_bucket_of(spec: &FooterSpec) -> FooterBucket`
- `pub fn footer_action_of(scope: FooterScope, spec: &FooterSpec) -> FooterAction`
- `pub fn parse_footer(lines: &[String]) -> FooterParseResult`
- `pub fn split_footer_line(line: &str) -> Vec<String>`
- `pub fn parse_footer_line(text: &str) -> Result<FooterSpec, String>`
- `pub fn parse_footer_options(text: &str) -> Result<BTreeMap<String, String>, String>`
- `pub fn split_inline_footer_options(raw: &str) -> (String, String)`
- `pub fn check_footer_placeholders(where_: &str, template: &str) -> Result<(), String>`
- `pub fn is_valid_style_option(value: &str) -> bool`
- `pub fn is_http_url(value: &str) -> bool`
- `pub fn footer_button_id(index: i32) -> String`
- `pub fn footer_default_visited(kind: FooterKind) -> &'static str`
- `pub fn footer_style(name: &str) -> ButtonStyle`
- `pub fn footer_button(spec: &FooterSpec, index: i32, original_input: &str) -> Option<Button>`
- `pub fn footer_buttons(specs: &[FooterSpec], original_input: &str) -> Vec<Button>`
- `pub fn footer_button_rows( …`
- `pub struct FooterHub`
- `impl FooterHub`
  - `pub fn new() -> Self`
  - `pub fn set_source(&self, scope: FooterScope, source: Option<FooterSource>)`
  - `pub fn set_lines(&self, scope: FooterScope, lines: Vec<String>)`
  - `pub fn is_enabled(&self, scope: FooterScope) -> bool`
  - `pub fn current_result(&self, scope: FooterScope) -> (FooterParseResult, Vec<String>)`
  - `pub fn merge( …`
  - `pub fn keyboard(&self, scope: FooterScope, original_input: &str) -> Option<Keyboard>`
  - `pub fn resolve_click(&self, data: &str) -> Result<FooterClick, FooterClickError>`
- `pub fn footer_count_key(scope: FooterScope, spec: &FooterSpec) -> String`
- `pub fn footer_button_id_count() -> i32`
- `pub fn footer_max_rows() -> usize`

## `src/keyboard.rs`

- `pub const DEFAULT_UNSUPPORTED_TIP: &str = "当前客户端不支持按钮"`
- `pub const MAX_ROWS: usize = 5`
- `pub const MAX_BUTTONS_PER_ROW: usize = 5`
- `pub const FONT_SIZE_SMALL: &str = "small"`
- `pub enum AllowedPermission`
- `impl AllowedPermission`
  - `pub const fn code(self) -> u8`
  - `pub const fn from_code(code: i32) -> Option<Self>`
- `pub enum ButtonAction`
- `impl ButtonAction`
  - `pub const fn code(self) -> u8`
  - `pub const fn from_code(code: i32) -> Option<Self>`
- `pub enum ButtonStyle`
- `impl ButtonStyle`
  - `pub const fn code(self) -> u8`
  - `pub const fn from_code(code: i32) -> Option<Self>`
- `pub struct RenderData`
- `pub struct Permission`
- `pub struct Action`
- `pub struct Button`
- `pub struct ButtonRow`
- `pub struct KeyboardStyle`
- `pub struct Keyboard`
- `pub enum CommandNormalizerState`
- `pub fn set_command_normalizer(hook: Option<TextHook>) -> CommandNormalizerState`
- `pub fn set_default_font_size(source: Option<Arc<dyn Fn() -> String + Send + Sync>>)`
- `pub fn normalize_font_size(size: &str) -> String`
- `impl Keyboard`
  - `pub fn new() -> Self`
  - `pub fn effective_style(&self) -> Option<KeyboardStyle>`
  - `pub fn set_font_size(&mut self, size: &str) -> &mut Self`
  - `pub fn append_button( …`
  - `pub fn append_row(&mut self, buttons: Vec<Button>) -> bool`
  - `pub fn remaining_rows(&self) -> usize`
  - `pub fn flatten(&self) -> Vec<Button>`
  - `pub fn has_button(&self, id: &str) -> bool`
  - `pub fn validate(&self) -> ButtonResult<()>`
  - `pub fn to_json(&self) -> ButtonResult<Vec<u8>>`
  - `pub fn merge_trailing(&mut self, per_row: usize, extra: Vec<Button>) -> &mut Self`
- `impl Action`
  - `pub fn action_data(&self) -> ButtonResult<String>`
- `impl Button`
  - `pub fn set_auto_command(&mut self, content: &str, auto_send: bool, anchor: bool) -> &mut Self`
  - `pub fn set_href(&mut self, url: &str) -> &mut Self`
  - `pub fn set_callback(&mut self, data: &str) -> &mut Self`
  - `pub fn set_permission(&mut self, required: AllowedPermission) -> &mut Self`
  - `pub fn set_user_white_list(&mut self, users: &[String]) -> &mut Self`
  - `pub fn set_unsupported_tip(&mut self, tip: &str) -> &mut Self`
  - `pub fn action_json(&self) -> ButtonResult<ActionJson>`
- `pub struct ActionJson`
- `impl RenderData`
- `pub fn qualify(scope: &str, id: &str) -> String`

## `src/lib.rs`

- `pub use builder::{auto_send, only, style, tip, visited, Builder, Opt}`
- `pub use callbacks::CallbackRegistry`
- `pub use error::{ButtonError, ButtonResult}`
- `pub use footer::{ check_footer_placeholders, footer_action_of, footer_bucket_of, footer_button, footer_button_id, footer_button_rows, footer_buttons, footer_count_key, footer_dedup_of, footer_slot_of, footer_style, parse_footer, parse_footer_line, parse_footer_options, split_footer_line, FooterAction, FooterBucket, FooterClick, FooterClickError, FooterDedup, FooterHub, FooterKind, FooterParseResult, FooterScope, FooterSpec, FOOTER_DEFAULT_REPLY, FOOTER_ID_PREFIX, FOOTER_MAX_BUTTONS, }`
- `pub use keyboard::{ normalize_font_size, qualify, set_command_normalizer, set_default_font_size, Action, AllowedPermission, Button, ButtonAction, ButtonRow, ButtonStyle, Keyboard, KeyboardStyle, Permission, RenderData, DEFAULT_UNSUPPORTED_TIP, FONT_SIZE_SMALL, MAX_BUTTONS_PER_ROW, MAX_ROWS, }`

