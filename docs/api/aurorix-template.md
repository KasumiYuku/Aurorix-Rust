# aurorix-template API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/args.rs`

- `pub enum Arg`
- `impl Arg`
  - `pub fn type_label(&self) -> &'static str`
- `pub type Args = BTreeMap<String, Arg>`
- `pub type FlatArgs = BTreeMap<String, String>`
- `pub fn arg_map<const N: usize>(pairs: [(&str, Arg); N]) -> Args`
- `macro_rules! args_map`
- `pub fn format_float(value: f64) -> String`
- `pub fn expand_decimal(mantissa: &str, exponent: i32) -> String`
- `pub fn flatten(source: &Args) -> TemplateResult<FlatArgs>`

## `src/engine.rs`

- `pub struct ProcessedTemplate`
- `pub fn process_template(input: &str) -> ProcessedTemplate`
- `pub fn fill(body: &str, args: &Args) -> TemplateResult<String>`
- `pub fn process_each(template: &str, args: &Args, flat: &FlatArgs) -> TemplateResult<String>`

## `src/error.rs`

- `pub enum TemplateError`
- `pub type TemplateResult<T> = Result<T, TemplateError>`

## `src/html.rs`

- `pub fn escape_html(input: &str) -> String`

## `src/inline.rs`

- `pub const INLINE_COMMAND_FUNC: &str = "inline"`
- `pub const INLINE_TEXT_LIMIT: usize = 100`
- `pub fn placeholder_regex() -> &'static Regex`
- `pub struct Operand`
- `pub struct InlineCall`
- `pub fn is_inline_call(tag: &str) -> bool`
- `pub fn inline_call_body(tag: &str) -> Option<&str>`
- `pub fn inline_arg_names(tag: &str) -> Vec<String>`
- `pub fn parse_inline_call(tag: &str) -> TemplateResult<InlineCall>`
- `pub fn render_inline_command(tag: &str, flat: &FlatArgs) -> TemplateResult<String>`
- `pub fn build_inline_command(command: &str, label: &str, reference: bool) -> String`
- `pub fn inline_truthy(value: &str) -> bool`
- `pub fn inline_length_exceeded(value: &str) -> bool`
- `pub fn resolve_inline_operand(operand: &Operand, flat: &FlatArgs) -> TemplateResult<String>`
- `pub fn parse_inline_operands(input: &str) -> TemplateResult<Vec<Operand>>`
- `pub fn process_inline_commands(template: &str, flat: &FlatArgs) -> TemplateResult<String>`

## `src/lib.rs`

- `pub use args::{arg_map, flatten, format_float, Arg, Args, FlatArgs}`
- `pub use engine::{fill, process_each, process_template, ProcessedTemplate}`
- `pub use error::{TemplateError, TemplateResult}`
- `pub use html::escape_html`
- `pub use inline::{ build_inline_command, inline_truthy, is_inline_call, render_inline_command, INLINE_TEXT_LIMIT, }`
- `pub use overrides::{ Overrides, ReloadHandle, DEFAULT_OVERRIDE_DIR, DEFAULT_RELOAD_SECONDS, GLOBAL_NAMESPACE, }`
- `pub use qqcard::{card_styles, is_qqcard_call, render_qq_card}`
- `pub use registry::{ fill_for, fill_html_for, fill_html_template, fill_markdown_template, html_template_count, is_html_template_exist, is_markdown_template_exist, markdown_template_count, new_html_template, new_markdown_template, register_builtin_templates, Registry, Template, TemplateKind, BUILTIN_CARD_TEMPLATE, }`
- `pub use url_codec::{encode_value, query_escape, query_unescape}`

## `src/overrides.rs`

- `pub const GLOBAL_NAMESPACE: &str = "_global"`
- `pub const DEFAULT_OVERRIDE_DIR: &str = "templates/override"`
- `pub const DEFAULT_RELOAD_SECONDS: u64 = 3`
- `pub struct Overrides`
- `pub struct ReloadHandle`
- `impl ReloadHandle`
  - `pub fn stop(&mut self)`
  - `pub fn is_running(&self) -> bool`
  - `pub fn overrides(&self) -> &Arc<Overrides>`
- `impl Overrides`
  - `pub fn new(registry: Arc<Registry>, root: impl Into<String>, interval: Duration) -> Self`
  - `pub fn with_global(registry: Arc<Registry>, root: impl Into<String>) -> Self`
  - `pub fn root(&self) -> &str`
  - `pub fn interval(&self) -> Duration`
  - `pub fn load(&self) -> io::Result<()>`
  - `pub fn last_reload(&self) -> Option<Instant>`
  - `pub fn reload_count(&self) -> u64`
  - `pub fn spawn_reloader(self: &Arc<Self>) -> ReloadHandle`

## `src/qqcard.rs`

- `pub const QQ_CARD_FUNC: &str = "qqcard"`
- `pub fn qq_card_params() -> BTreeMap<&'static str, Vec<&'static str>>`
- `pub fn card_styles() -> Vec<&'static str>`
- `pub fn is_qqcard_call(tag: &str) -> bool`
- `pub fn qq_card_call_body(tag: &str) -> Option<&str>`
- `pub struct NamedArg`
- `pub fn qq_card_arg_names(tag: &str) -> Vec<String>`
- `pub fn render_qq_card(tag: &str, flat: &FlatArgs) -> TemplateResult<String>`
- `pub fn split_leading_token(body: &str) -> TemplateResult<(String, &str)>`
- `pub fn parse_named_args(input: &str) -> TemplateResult<Vec<NamedArg>>`

## `src/registry.rs`

- `pub enum TemplateKind`
- `impl TemplateKind`
  - `pub const fn suffix(self) -> &'static str`
  - `pub const fn label(self) -> &'static str`
- `pub struct Template`
- `impl Template`
  - `pub fn new(kind: TemplateKind, id: &str, content: &str) -> Self`
  - `pub fn id(&self) -> &str`
  - `pub fn body(&self) -> &str`
  - `pub fn args(&self) -> &[String]`
  - `pub fn kind(&self) -> TemplateKind`
- `pub struct Registry`
- `impl Registry`
  - `pub fn new() -> Self`
  - `pub fn global() -> &'static Registry`
  - `pub fn register(&self, kind: TemplateKind, ns: &str, id: &str, content: &str) -> bool`
  - `pub fn drop_template(&self, kind: TemplateKind, ns: &str, id: &str)`
  - `pub fn base_of(&self, kind: TemplateKind, ns: &str, id: &str) -> Option<Arc<str>>`
  - `pub fn exists(&self, kind: TemplateKind, id: &str) -> bool`
  - `pub fn count(&self, kind: TemplateKind) -> usize`
  - `pub fn ids(&self, kind: TemplateKind, ns: &str) -> Vec<String>`
  - `pub fn get(&self, kind: TemplateKind, ns: &str, id: &str) -> Option<Arc<Template>>`
  - `pub fn fill( …`
  - `pub fn register_files<I, N, C>(&self, kind: TemplateKind, ns: &str, files: I) -> usize`
- `pub fn new_markdown_template(id: &str, content: &str)`
- `pub fn new_html_template(id: &str, content: &str)`
- `pub const BUILTIN_CARD_TEMPLATE: &str = include_str!("../assets/builtin/Card.md")`
- `pub fn register_builtin_templates(registry: &Registry) -> usize`
- `pub fn is_markdown_template_exist(id: &str) -> bool`
- `pub fn is_html_template_exist(id: &str) -> bool`
- `pub fn fill_markdown_template(id: &str, args: &Args) -> TemplateResult<String>`
- `pub fn fill_for(ns: &str, id: &str, args: &Args) -> TemplateResult<String>`
- `pub fn fill_html_template(id: &str, args: &Args) -> TemplateResult<String>`
- `pub fn fill_html_for(ns: &str, id: &str, args: &Args) -> TemplateResult<String>`
- `pub fn markdown_template_count() -> usize`
- `pub fn html_template_count() -> usize`

## `src/url_codec.rs`

- `pub fn query_escape(input: &str) -> String`
- `pub fn query_unescape(input: &str) -> Option<String>`
- `pub fn encode_value(value: &str) -> String`

