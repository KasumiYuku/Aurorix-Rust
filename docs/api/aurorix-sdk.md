# aurorix-sdk API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/context.rs`

- `pub struct Context`
- `impl Context`
  - `pub fn new(scene: Scene, target: Target) -> Self`
  - `pub fn with_user(mut self, user: impl Into<String>) -> Self`
- `pub trait Extract: Sized`
- `pub fn extract<T: Extract>(ctx: &Context) -> Result<T, SdkError>`

## `src/error.rs`

- `pub enum SdkError`
- `pub type SdkResult<T> = Result<T, SdkError>`

## `src/facade.rs`

- `pub fn render(registry: &Registry, id: &str, args: Args) -> Result<String, SdkError>`
- `pub fn render_for(registry: &Registry, ns: &str, id: &str, args: Args) -> Result<String, SdkError>`
- `pub fn keyboard(plugin_id: &str) -> Builder`

## `src/lib.rs`

- `pub use context::{extract, Context, Extract}`
- `pub use error::{SdkError, SdkResult}`
- `pub use facade::{keyboard, render, render_for}`
- `pub use plugin::{Plugin, PluginMeta}`
- `pub use registry::PluginRegistry`
- `pub use aurorix_buttons::Builder`
- `pub use aurorix_core::{Scene, Target}`
- `pub use aurorix_protocol`
- `pub use aurorix_template::{Args, Registry}`

## `src/plugin.rs`

- `pub struct PluginMeta`
- `impl PluginMeta`
  - `pub const UNKNOWN_VERSION: &'static str = "unknown"`
  - `pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self`
- `pub trait Plugin: Send + Sync`

## `src/registry.rs`

- `pub struct PluginRegistry`
- `impl PluginRegistry`
  - `pub fn new() -> Self`
  - `pub fn global() -> &'static Self`
  - `pub fn register(&self, plugin: Box<dyn Plugin>) -> Option<PluginMeta>`
  - `pub fn get(&self, id: &str) -> Option<Arc<dyn Plugin>>`
  - `pub fn meta(&self, id: &str) -> Option<PluginMeta>`
  - `pub fn ids(&self) -> Vec<String>`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn contains(&self, id: &str) -> bool`

