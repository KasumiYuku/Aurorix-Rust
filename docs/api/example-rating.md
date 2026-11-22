# example-rating API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/covers.rs`

- `pub const COVER_SIZE: i32 = 256`
- `pub fn cover_image(index: usize) -> Option<Image>`

## `src/lib.rs`

- `pub use covers::{cover_image, COVER_SIZE}`
- `pub use sample::{ emit_sample_outputs, sample_output_dir, sample_output_dir_from, sample_output_paths, SampleOutput, SAMPLE_ENV, SAMPLE_PNG_NAME, SAMPLE_WEBP_NAME, }`
- `pub const CARD_WIDTH: i32 = 900`
- `pub const CARD_HEIGHT: i32 = 1200`
- `pub const CARD_SCALE: f32 = 2.0`
- `pub const CARD_MARGIN: f32 = 48.0`
- `pub const HEADER_HEIGHT: f32 = 196.0`
- `pub const ROWS_TOP: f32 = 210.0`
- `pub const ROWS_BOTTOM: f32 = 1050.0`
- `pub const ROW_GAP: f32 = 12.0`
- `pub const MAX_ROWS: usize = 10`
- `pub const MIN_ROW_HEIGHT: f32 = 56.0`
- `pub const MAX_ROW_HEIGHT: f32 = 96.0`
- `pub const FOOTER_TOP: f32 = 1074.0`
- `pub const FOOTER_HEIGHT: f32 = 64.0`
- `pub const PLUGIN_ID: &str = "rating"`
- `pub const COMMAND: &str = "b30"`
- `pub const DEFAULT_TOP: usize = 10`
- `pub const FONT_SANS: &str = "DejaVu Sans"`
- `pub const FONT_MONO: &str = "DejaVu Sans Mono"`
- `pub const COVER_ASSET_KEY: &str = "covers/star.png"`
- `pub struct Score`
- `impl Score`
  - `pub fn new(title: impl Into<String>, difficulty: impl Into<String>, rating: f64) -> Self`
- `pub trait Best30Store: Send + Sync`
- `pub struct MemoryStore`
- `impl MemoryStore`
  - `pub fn new() -> Self`
  - `pub fn seed(&self, user: impl Into<String>, scores: Vec<Score>) -> &Self`
- `pub fn average_rating(scores: &[Score], top: usize) -> f64`
- `pub fn card_font_chain() -> FontChain`
- `pub fn row_height(count: usize) -> f32`
- `pub fn row_rect(index: usize, count: usize) -> Rect`
- `pub fn pill_anchor(index: usize, count: usize) -> (f32, f32, f32)`
- `pub fn plugin(store: Arc<dyn Best30Store>) -> PluginCommands`
- `pub fn command_spec(store: Arc<dyn Best30Store>) -> CommandSpec`
- `pub fn draw_card(kit: &CanvasKit<'_>, user: &str, scores: &[Score], average: f64)`
- `pub fn draw_card_with_assets( …`
- `pub fn demo_scores() -> Vec<Score>`

## `src/metrics.rs`

- `pub const TEXT_BAND: Band = Band::new(48, 64, 352, 44)`
- `pub const BACKGROUND_BAND: Band = Band::new(48, 200, 804, 8)`
- `pub const PILL_SCAN_WIDTH: usize = 140`
- `pub struct Band`
- `impl Band`
  - `pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self`
  - `pub fn scaled(self, scale: f32) -> Self`
  - `pub fn clamp_to(self, width: usize, height: usize) -> Self`
- `pub struct BandStats`
- `pub struct Analysis`
- `pub struct RgbaImage`
- `impl RgbaImage`
  - `pub fn decode(bytes: &[u8]) -> Option<Self>`
  - `pub fn pixel(&self, x: usize, y: usize) -> [u8; 4]`
  - `pub fn luminance(&self, x: usize, y: usize) -> f64`
  - `pub fn band_stats(&self, band: Band) -> BandStats`
- `pub fn pill_band(scale: f32) -> Band`
- `pub fn analyze(image: &RgbaImage, scale: f32) -> Analysis`

## `src/sample.rs`

- `pub const SAMPLE_ENV: &str = "AURORIX_SAMPLE_OUT"`
- `pub const SAMPLE_PNG_NAME: &str = "sample-b30-card.png"`
- `pub const SAMPLE_WEBP_NAME: &str = "sample-b30-card.webp"`
- `pub struct SampleOutput`
- `pub fn sample_output_dir_from(value: Option<&OsStr>) -> Option<PathBuf>`
- `pub fn sample_output_dir() -> Option<PathBuf>`
- `pub fn sample_output_paths(dir: &Path) -> (PathBuf, PathBuf)`
- `pub fn emit_sample_outputs( …`

