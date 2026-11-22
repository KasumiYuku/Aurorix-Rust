# aurorix-render API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/asset.rs`

- `pub trait AssetLoader: Send + Sync`

## `src/backend.rs`

- `pub struct BackendCapabilities`
- `pub trait RenderBackend: Send + Sync`
- `pub type BackendHandle = Arc<dyn RenderBackend>`
- `pub struct PlaceholderBackend`
- `impl PlaceholderBackend`
  - `pub fn new() -> Self`
  - `pub fn handle() -> BackendHandle`
- `pub struct SkiaBackend`
- `impl SkiaBackend`
  - `pub fn new() -> Self`
  - `pub fn with_font_manager(fonts: Arc<SharedFontMgr>) -> Self`
  - `pub fn handle() -> BackendHandle`
  - `pub fn font_manager(&self) -> Arc<SharedFontMgr>`
  - `pub fn font_family_count(&self) -> usize`

## `src/batch.rs`

- `pub struct Sprite`
- `impl Sprite`
  - `pub fn new(src: Rect, dest: Rect) -> Self`
  - `pub fn from_size(src: Rect, x: f32, y: f32, width: f32, height: f32) -> Self`
  - `pub fn scale(&self) -> (f32, f32)`
- `pub struct PlacedText`
- `impl PlacedText`
  - `pub fn new(text: impl Into<String>, x: f32, y: f32) -> Self`
- `pub struct TextBlobKey`
- `impl TextBlobKey`
  - `pub fn new(family: &str, size: u32, bold: bool, text: &str) -> Self`
  - `pub fn family(&self) -> &str`
  - `pub fn size(&self) -> u32`
  - `pub fn bold(&self) -> bool`
  - `pub fn text(&self) -> &str`
- `pub struct TextBlobStats`
- `impl BlobState`
- `pub struct TextBlobCache`
- `impl TextBlobCache`
  - `pub fn new(capacity: usize) -> Self`
  - `pub fn capacity(&self) -> usize`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn get(&self, key: &TextBlobKey) -> Option<TextBlob>`
  - `pub fn insert(&self, key: TextBlobKey, blob: TextBlob) -> TextBlob`
  - `pub fn clear(&self)`
  - `pub fn stats(&self) -> TextBlobStats`

## `src/cache.rs`

- `pub struct CacheStats`
- `pub struct Texture`
- `impl Texture`
  - `pub fn key(&self) -> &str`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn encoded(&self) -> &Bytes`
  - `pub fn image(&self) -> Option<&Image>`
  - `pub fn decoded(&self) -> Option<Arc<SharedImage>>`
  - `pub fn dimensions(&self) -> Option<(i32, i32)>`
- `impl TextureState`
- `pub struct TextureCache`
- `impl TextureCache`
  - `pub fn new(capacity_bytes: u64) -> Self`
  - `pub fn capacity_bytes(&self) -> u64`
  - `pub fn insert(&self, key: &str, bytes: Bytes) -> Result<Arc<Texture>, RenderError>`
  - `pub fn insert_encoded(&self, key: &str, bytes: Bytes) -> Result<Arc<Texture>, RenderError>`
  - `pub fn image_for(&self, bytes: Bytes) -> Result<Arc<Texture>, RenderError>`
  - `pub fn decodes(&self) -> u64`
  - `pub fn drop_decoded(&self) -> usize`
  - `pub fn get(&self, key: &str) -> Option<Arc<Texture>>`
  - `pub fn remove(&self, key: &str)`
  - `pub fn invalidate_all(&self)`
  - `pub fn stats(&self) -> CacheStats`
- `pub fn content_key(bytes: &[u8]) -> String`

## `src/codec.rs`

- `pub const DEFAULT_QUALITY: u8 = 85`
- `pub struct EncodeOptions`
- `impl EncodeOptions`
  - `pub fn new(quality: u8) -> Self`
  - `pub fn with_quality(mut self, quality: u8) -> Self`
  - `pub fn with_lossless(mut self, lossless: bool) -> Self`
  - `pub fn with_webp_method(mut self, method: u8) -> Self`
  - `pub fn with_jpeg_progressive(mut self, progressive: bool) -> Self`
  - `pub fn from_request(request: &RenderRequest) -> Self`
  - `pub fn validate(&self) -> Result<(), RenderError>`
  - `pub fn effective_webp_method(&self) -> u8`
- `pub enum RenderQuality`
- `impl RenderQuality`
  - `pub const MAX_QUALITY: u8 = 100`
  - `pub const HIGH_QUALITY: u8 = 92`
  - `pub const WEBP_METHOD_BEST: u8 = 6`
  - `pub fn name(self) -> &'static str`
  - `pub fn apply(self, request: RenderRequest) -> RenderRequest`
  - `pub fn encode_options(self) -> EncodeOptions`
- `pub fn default_webp_method(quality: u8) -> u8`
- `pub fn encode_surface( …`
- `pub fn encode_rgba( …`
- `pub fn skia_format(format: RenderFormat) -> EncodedImageFormat`

## `src/context.rs`

- `pub struct FontHandle`
- `impl FontKey`
- `impl FontSlot`
- `impl TypefaceKey`
- `impl TypefaceSlot`
- `pub struct FontBook`
- `impl FontBook`
  - `pub fn new(capacity: usize, manager: Option<Arc<SharedFontMgr>>) -> Self`
  - `pub fn capacity(&self) -> usize`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn manager(&self) -> Option<&Arc<SharedFontMgr>>`
  - `pub fn family_count(&self) -> usize`
  - `pub fn get(&self, family: &str, size: u32) -> Option<FontHandle>`
  - `pub fn load( …`
  - `pub fn typeface(&self, family: &str, bold: bool) -> Result<Arc<SharedTypeface>, RenderError>`
  - `pub fn registered_families(&self) -> Vec<String>`
  - `pub fn registered_family_count(&self) -> usize`
  - `pub fn has_registered_family(&self, family: &str) -> bool`
  - `pub fn fallback_chain(&self, family: &str, bold: bool) -> Vec<Arc<SharedTypeface>>`
- `pub struct RenderContext`
- `impl RenderContext`
  - `pub fn new(backend: BackendHandle, font_capacity: usize, texture_capacity: u64) -> Arc<Self>`
  - `pub fn pressure(&self) -> MemoryPressure`
  - `pub fn emergency_reclaim(&self) -> ReclaimReport`
  - `pub fn backend(&self) -> &BackendHandle`
  - `pub fn fonts(&self) -> &FontBook`
  - `pub fn textures(&self) -> &TextureCache`
  - `pub fn text_blobs(&self) -> &TextBlobCache`
  - `pub fn draw<F>(&self, request: RenderRequest, painter: F) -> Result<Bytes, RenderError>`
  - `pub fn with_asset_loader(self: &Arc<Self>, loader: Arc<dyn AssetLoader>) -> Arc<Self>`
  - `pub fn assets(&self) -> Option<&Arc<dyn AssetLoader>>`
  - `pub fn has_asset_loader(&self) -> bool`
  - `pub fn asset(&self, key: &str) -> Result<Arc<Texture>, RenderError>`
  - `pub fn draw_with_options<F>( …`
  - `pub fn draw_text( …`
  - `pub fn measure_text(&self, text: &str, style: &TextStyle) -> Result<f32, RenderError>`

## `src/draw.rs`

- `pub struct Color`
- `impl Color`
  - `pub const WHITE: Self = Self::new(255, 255, 255, 255)`
  - `pub const BLACK: Self = Self::new(0, 0, 0, 255)`
  - `pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0)`
  - `pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self`
  - `pub const fn rgb(r: u8, g: u8, b: u8) -> Self`
  - `pub const fn with_alpha(self, a: u8) -> Self`
  - `pub fn to_skia(self) -> skia_safe::Color`
- `pub enum RenderFormat`
- `impl RenderFormat`
  - `pub fn name(self) -> &'static str`
  - `pub fn mime(self) -> &'static str`
  - `pub fn is_opaque(self) -> bool`
  - `pub fn to_skia(self) -> EncodedImageFormat`
- `pub struct RenderRequest`
- `impl RenderRequest`
  - `pub const DEFAULT_QUALITY: u8 = 90`
  - `pub const MIN_SCALE: f32 = 1.0`
  - `pub const MAX_SCALE: f32 = 4.0`
  - `pub fn new(width: i32, height: i32) -> Self`
  - `pub fn with_format(mut self, format: RenderFormat) -> Self`
  - `pub fn with_quality(mut self, quality: u8) -> Self`
  - `pub fn with_background(mut self, background: Color) -> Self`
  - `pub fn with_scale(mut self, scale: f32) -> Self`
  - `pub fn clamp_scale(scale: f32) -> f32`
  - `pub fn effective_scale(&self) -> f32`
  - `pub fn physical_size(&self) -> (i32, i32)`
  - `pub fn effective_background(&self) -> Option<Color>`
  - `pub fn estimated_bytes(&self) -> u64`
- `pub enum FontWeight`
- `impl FontWeight`
  - `pub fn value(self) -> u16`
  - `pub fn is_bold(self) -> bool`
- `pub enum TextAlign`
- `pub struct TextStyle`
- `impl TextStyle`
  - `pub fn new(family: impl Into<String>, size: u32) -> Self`
  - `pub fn bold(&self) -> bool`
  - `pub fn with_bold(mut self, bold: bool) -> Self`
  - `pub fn with_weight(mut self, weight: FontWeight) -> Self`
  - `pub fn with_color(mut self, color: Color) -> Self`
  - `pub fn with_size(mut self, size: u32) -> Self`
  - `pub fn with_letter_spacing(mut self, letter_spacing: f32) -> Self`
  - `pub fn with_align(mut self, align: TextAlign) -> Self`
- `pub fn crisp_font(mut font: Font) -> Font`
- `pub fn text_paint(color: Color) -> Paint`
- `pub fn font_for(fonts: &FontBook, style: &TextStyle) -> Result<Font, RenderError>`
- `pub fn measure_text(fonts: &FontBook, text: &str, style: &TextStyle) -> Result<f32, RenderError>`
- `pub fn draw_text( …`
- `pub fn fill(canvas: &Canvas, color: Color)`
- `pub fn fill_rect(canvas: &Canvas, left: f32, top: f32, width: f32, height: f32, color: Color)`

## `src/error.rs`

- `pub enum RenderError`
- `impl RenderError`
  - `pub fn enqueue_full(capacity: usize) -> Self`
  - `pub fn encode(message: impl Into<String>) -> Self`
- `pub type RenderResult<T> = Result<T, RenderError>`

## `src/fonts.rs`

- `pub const TRUETYPE_SIGNATURE: [u8; 4] = [0x00, 0x01, 0x00, 0x00]`
- `pub const CFF_SIGNATURE: [u8; 4] = *b"OTTO"`
- `pub const COLLECTION_SIGNATURE: [u8; 4] = *b"ttcf"`
- `pub const COLLECTION_FACE_LIMIT: usize = 256`
- `pub enum FontContainer`
- `impl FontContainer`
  - `pub const fn signature(self) -> [u8; 4]`
  - `pub const fn label(self) -> &'static str`
  - `pub const fn is_collection(self) -> bool`
- `pub fn font_container(data: &[u8]) -> Option<FontContainer>`
- `pub fn collection_header(data: &[u8]) -> Option<usize>`
- `pub fn collection_alias(family: &str, index: usize) -> String`
- `pub fn collection_face_index(family: &str) -> usize`
- `pub fn collection_face_count(data: &[u8]) -> usize`
- `pub struct FontFace`
- `impl FontFace`
  - `pub fn bytes(&self, bold: bool) -> Arc<Vec<u8>>`
  - `pub fn has_bold(&self) -> bool`
  - `pub fn container(&self) -> Option<FontContainer>`
  - `pub fn bold_container(&self) -> Option<FontContainer>`
- `pub fn normalize_family(family: &str) -> String`
- `pub fn register_font_family( …`
- `pub fn register_font_collection(family: &str, bytes: &[u8]) -> usize`
- `pub fn font_family_bytes(family: &str, bold: bool) -> Option<Arc<Vec<u8>>>`
- `pub fn font_face(family: &str) -> Option<FontFace>`
- `pub fn has_font_family(family: &str) -> bool`
- `pub fn font_family_names() -> Vec<String>`
- `pub fn font_family_count() -> usize`
- `pub fn clear_font_families()`
- `pub fn data_font_manager() -> &'static SharedFontMgr`

## `src/gpu.rs`

- `pub const GPU_ENV: &str = "AURORIX_GPU"`
- `pub const GPU_FEATURE_ENABLED: bool = cfg!(feature = "gpu")`
- `pub enum GpuMode`
- `impl GpuMode`
  - `pub const fn as_str(self) -> &'static str`
  - `pub fn parse(raw: &str) -> Option<Self>`
  - `pub fn from_env() -> Self`
- `pub enum GpuDeviceKind`
- `impl GpuDeviceKind`
  - `pub const fn label(self) -> &'static str`
- `pub struct GpuInfo`
- `impl GpuInfo`
  - `pub fn label(&self) -> String`
  - `pub fn api_version_text(&self) -> String`
- `pub enum GpuSupport`
- `impl GpuSupport`
  - `pub fn is_available(&self) -> bool`
  - `pub fn is_disabled(&self) -> bool`
  - `pub fn info(&self) -> Option<&GpuInfo>`
  - `pub fn reason(&self) -> Option<&str>`
  - `pub fn describe(&self) -> String`
- `pub struct GpuRuntimeState`
- `impl GpuRuntimeState`
  - `pub fn hardware_accelerated(&self) -> bool`
  - `pub fn describe(&self) -> String`
- `pub fn decode_version(raw: u32) -> (u32, u32, u32)`
- `pub fn probe_with(mode: GpuMode) -> GpuSupport`
- `pub fn probe() -> GpuSupport`
- `pub fn cached() -> &'static GpuSupport`
- `pub fn hardware_accelerated() -> bool`
- `pub fn is_usable() -> bool`
- `pub fn runtime_state() -> GpuRuntimeState`
- `pub fn note_failure(reason: &str)`
- `pub fn fallback_count() -> u64`
- `pub fn reset_failure() -> bool`
- `pub fn ensure_ready() -> Result<(), String>`
- `pub fn render_into<R>( …`
- `pub const API: &str = "vulkan"`

## `src/handle.rs`

- `pub fn font_style(bold: bool) -> FontStyle`
- `pub struct SharedFontMgr(FontMgr)`
- `impl SharedFontMgr`
  - `pub fn system() -> Self`
  - `pub fn raw(&self) -> &FontMgr`
  - `pub fn family_count(&self) -> usize`
  - `pub fn typeface_from_data(&self, data: &[u8]) -> Option<SharedTypeface>`
  - `pub fn typeface_at(&self, data: &[u8], index: usize) -> Option<SharedTypeface>`
  - `pub fn typeface_from_collection(&self, data: &[u8]) -> Vec<SharedTypeface>`
  - `pub fn match_family(&self, family: &str, bold: bool) -> Option<SharedTypeface>`
  - `pub fn default_typeface(&self, bold: bool) -> Option<SharedTypeface>`
- `pub struct SharedTypeface(Typeface)`
- `impl SharedTypeface`
  - `pub fn from_typeface(typeface: Typeface) -> Self`
  - `pub fn typeface(&self) -> &Typeface`
  - `pub fn duplicate(&self) -> Typeface`
  - `pub fn family_name(&self) -> String`
- `pub struct SharedImage(Image)`
- `impl SharedImage`
  - `pub fn from_image(image: Image) -> Self`
  - `pub fn image(&self) -> &Image`
  - `pub fn duplicate(&self) -> Image`
  - `pub fn dimensions(&self) -> (i32, i32)`

## `src/kit.rs`

- `pub const TEXT_BLOB_MAX_BYTES: usize = 64`
- `pub const DEFAULT_FALLBACKS: [&str; 6] = [`
- `pub struct FontChain`
- `impl FontChain`
  - `pub fn new(primary: impl Into<String>) -> Self`
  - `pub fn with_default_fallbacks(primary: impl Into<String>) -> Self`
  - `pub fn with_fallback(mut self, family: impl Into<String>) -> Self`
  - `pub fn with_fallbacks<I, S>(mut self, families: I) -> Self`
  - `pub fn primary(&self) -> &str`
  - `pub fn fallbacks(&self) -> &[String]`
  - `pub fn families(&self) -> Vec<String>`
- `pub struct GradientStop`
- `impl GradientStop`
  - `pub fn new(offset: f32, color: Color) -> Self`
- `pub struct LinearGradient`
- `impl LinearGradient`
  - `pub fn new<I>(stops: I) -> Self`
  - `pub fn horizontal<I>(stops: I) -> Self`
  - `pub fn vertical<I>(stops: I) -> Self`
  - `pub fn with_angle(mut self, degrees: f32) -> Self`
  - `pub fn with_points(mut self, from: impl Into<Point>, to: impl Into<Point>) -> Self`
  - `pub fn with_stop(mut self, stop: GradientStop) -> Self`
  - `pub fn shader(&self, bounds: Rect) -> Result<Shader, RenderError>`
- `pub struct RadialGradient`
- `impl RadialGradient`
  - `pub fn new<I>(stops: I) -> Self`
  - `pub fn with_center(mut self, center: impl Into<Point>) -> Self`
  - `pub fn with_radius(mut self, radius: f32) -> Self`
  - `pub fn shader(&self, bounds: Rect) -> Result<Shader, RenderError>`
- `pub enum Gradient`
- `impl Gradient`
  - `pub fn shader(&self, bounds: Rect) -> Result<Shader, RenderError>`
- `pub struct Border`
- `impl Border`
  - `pub fn new(color: Color, width: f32) -> Self`
- `pub struct Padding`
- `impl Padding`
  - `pub fn new(x: f32, y: f32) -> Self`
  - `pub fn all(value: f32) -> Self`
- `pub struct TextMetrics`
- `impl TextMetrics`
  - `pub fn as_tuple(&self) -> (f32, f32, f32)`
- `pub enum FitMode`
- `impl FitMode`
- `pub fn default_sampling() -> SamplingOptions`
- `pub fn sampling_crisp() -> SamplingOptions`
- `pub fn sampling_cubic() -> SamplingOptions`
- `pub fn sampling_linear() -> SamplingOptions`
- `pub fn sampling_nearest() -> SamplingOptions`
- `pub struct TextLayout`
- `impl TextLayout`
  - `pub fn primary_font(&self) -> &Font`
  - `pub fn primary_covers(&self, text: &str) -> bool`
- `pub trait ImageSource`
- `pub struct CanvasKit<'a>`
- `impl CanvasKit<'a>`
  - `pub fn new(canvas: &'a Canvas, context: &'a RenderContext) -> Self`
  - `pub fn with_chain(canvas: &'a Canvas, context: &'a RenderContext, chain: FontChain) -> Self`
  - `pub fn with_sampling(mut self, sampling: SamplingOptions) -> Self`
  - `pub fn sampling(&self) -> SamplingOptions`
  - `pub fn canvas(&self) -> &'a Canvas`
  - `pub fn fonts(&self) -> &'a FontBook`
  - `pub fn textures(&self) -> &'a TextureCache`
  - `pub fn asset(&self, key: &str) -> Result<Arc<Texture>, RenderError>`
  - `pub fn chain(&self) -> &FontChain`
  - `pub fn draw_image( …`
  - `pub fn draw_image_fit( …`
  - `pub fn draw_round_image( …`
  - `pub fn layer<R>( …`
  - `pub fn blur_layer<R>(&self, sigma: f32, content: impl FnOnce(&Self) -> R) -> R`
  - `pub fn blur_layer_in<R>( …`
  - `pub fn drop_shadow<R>( …`
  - `pub fn drop_shadow_in<R>( …`
  - `pub fn layer_bounds(&self) -> Rect`
  - `pub fn rrect_fill(&self, rect: Rect, radius: f32, color: Color) -> &'a Canvas`
  - `pub fn fill_rect(&self, rect: Rect, color: Color) -> &'a Canvas`
  - `pub fn fill_gradient( …`
  - `pub fn stroke(&self, rect: Rect, radius: f32, border: Border) -> &'a Canvas`
  - `pub fn circle(&self, center: impl Into<Point>, radius: f32, color: Color) -> &'a Canvas`
  - `pub fn pill( …`
  - `pub fn rounded_image(&self, image: &Image, rect: Rect, radius: f32) -> &'a Canvas`
  - `pub fn rounded_image_with_sampling( …`
  - `pub fn image_rect(&self, image: &Image, rect: Rect) -> &'a Canvas`
  - `pub fn image_rect_with_sampling( …`
  - `pub fn cover_image(&self, image: &Image, rect: Rect) -> &'a Canvas`
  - `pub fn cover_image_with_sampling( …`
  - `pub fn draw_asset(&self, key: &str, rect: Rect, radius: f32) -> Result<(), RenderError>`
  - `pub fn draw_asset_cover(&self, key: &str, rect: Rect, radius: f32) -> Result<(), RenderError>`
  - `pub fn fill_rects_batch(&self, rects: &[Rect], color: Color) -> &'a Canvas`
  - `pub fn fill_round_rects_batch(&self, items: &[(Rect, f32)], color: Color) -> &'a Canvas`
  - `pub fn draw_sprites_batch(&self, image: &Image, sprites: &[Sprite]) -> &'a Canvas`
  - `pub fn draw_texts_batch( …`
  - `pub fn text_blob_cache(&self) -> &'a TextBlobCache`
  - `pub fn text_layout(&self, style: &TextStyle) -> Result<TextLayout, RenderError>`
  - `pub fn measure(&self, text: &str, style: &TextStyle) -> Result<TextMetrics, RenderError>`
  - `pub fn draw_text( …`
  - `pub fn truncate_with_ellipsis(&self, text: &str, style: &TextStyle, max_width: f32) -> String`
  - `pub fn fit_to_width( …`
  - `pub fn fit_to_width_with( …`
  - `pub fn wrap(&self, text: &str, style: &TextStyle, max_width: f32) -> Vec<String>`
  - `pub fn draw_paragraph( …`
  - `pub fn badge_size(&self, text: &str, style: &TextStyle, padding: Padding) -> (f32, f32)`
  - `pub fn badge( …`
  - `pub fn badge_at( …`
- `pub fn rounded(rect: Rect, radius: f32) -> RRect`
- `pub fn solid_paint(color: Color) -> Paint`
- `pub fn cover_source_rect(image: &Image, rect: Rect) -> Rect`
- `pub fn weight_of(weight: FontWeight) -> u16`
- `pub fn contain_destination_rect(image: &Image, rect: Rect) -> Rect`

## `src/layer.rs`

- `pub struct RecordedLayer`
- `impl RecordedLayer`
  - `pub fn record(bounds: Rect, content: impl FnOnce(&Canvas)) -> Result<Self, RenderError>`
  - `pub fn bounds(&self) -> Rect`
  - `pub fn picture(&self) -> &Picture`
  - `pub fn draw<'c>(&self, canvas: &'c Canvas) -> &'c Canvas`
  - `pub fn draw_at<'c>(&self, canvas: &'c Canvas, offset: Point) -> &'c Canvas`
  - `pub fn draw_scaled<'c>(&self, canvas: &'c Canvas, scale: f32) -> &'c Canvas`

## `src/lib.rs`

- `pub use asset::AssetLoader`
- `pub use backend::{ BackendCapabilities, BackendHandle, PlaceholderBackend, RenderBackend, SkiaBackend, }`
- `pub use batch::{PlacedText, Sprite, TextBlobCache, TextBlobKey, TextBlobStats}`
- `pub use cache::{content_key, CacheStats, Texture, TextureCache}`
- `pub use codec::{default_webp_method, encode_rgba, encode_surface, EncodeOptions, RenderQuality}`
- `pub use context::{FontBook, FontHandle, RenderContext}`
- `pub use draw::{ crisp_font, draw_text, fill, fill_rect, measure_text, text_paint, Color, FontWeight, RenderFormat, RenderRequest, TextAlign, TextStyle, }`
- `pub use error::{RenderError, RenderResult}`
- `pub use fonts::{ clear_font_families, collection_alias, collection_face_count, collection_face_index, collection_header, data_font_manager, font_container, font_face, font_family_bytes, font_family_count, font_family_names, has_font_family, normalize_family, register_font_collection, register_font_family, FontContainer, FontFace, COLLECTION_FACE_LIMIT, }`
- `pub use gpu::{ decode_version, fallback_count, hardware_accelerated, note_failure, probe, probe_with, reset_failure, runtime_state, GpuDeviceKind, GpuInfo, GpuMode, GpuRuntimeState, GpuSupport, GPU_ENV, GPU_FEATURE_ENABLED, }`
- `pub use handle::{SharedFontMgr, SharedImage, SharedTypeface}`
- `pub use kit::{ contain_destination_rect, cover_source_rect, default_sampling, sampling_crisp, sampling_cubic, sampling_linear, sampling_nearest, solid_paint, Border, CanvasKit, FitMode, FontChain, Gradient, GradientStop, ImageSource, LinearGradient, Padding, RadialGradient, TextLayout, TextMetrics, TEXT_BLOB_MAX_BYTES, }`
- `pub use layer::RecordedLayer`
- `pub use memory::{ after_render, before_render, emergency_reclaim, pressure, reclaim, ReclaimReport, }`
- `pub use plan::{ current_plan, detect_plan, detect_pool_config, detect_scheduler_config, detect_workers, surface_cache_budget, RendererPlan, }`
- `pub use pool::{ is_pool_worker, reset_surface_reuse_stats, surface_reuse_stats, thread_reuse_stats, PoolJob, PoolStats, PoolTicket, RenderPool, RenderPoolConfig, SurfaceReuseStats, }`
- `pub use scheduler::{RenderHandle, RenderScheduler, SchedulerConfig, SchedulerMetrics}`
- `pub use skia_safe`
- `pub use task::{RenderJob, RenderOutput, RenderPriority, RenderTask, RenderTicket}`
- `pub use webp`

## `src/memory.rs`

- `pub const PURGE_EVERY: u64 = 8`
- `pub const RECLAIM_EVERY: u64 = 64`
- `pub struct ReclaimReport`
- `impl ReclaimReport`
  - `pub fn total(self) -> usize`
- `pub fn configure_caches()`
- `pub fn register_texture_cache(cache: &Arc<TextureCache>)`
- `pub fn register_text_blob_cache(cache: &Arc<TextBlobCache>)`
- `pub fn purge_textures() -> usize`
- `pub fn purge_text_blobs() -> usize`
- `pub fn reclaim() -> usize`
- `pub fn purge_caches()`
- `pub fn trim_memory() -> bool`
- `pub fn pressure() -> MemoryPressure`
- `pub fn emergency_reclaim() -> ReclaimReport`
- `pub fn before_render() -> MemoryPressure`
- `pub fn after_render()`
- `pub fn renders() -> u64`
- `pub fn surface_epoch() -> u64`
- `pub fn bump_surface_epoch() -> u64`
- `pub fn surface_cache_limit() -> usize`
- `pub fn font_cache_limit() -> usize`
- `pub fn resource_cache_limit() -> usize`
- `pub fn text_blob_capacity() -> usize`

## `src/plan.rs`

- `pub const MIN_WORKERS: usize = 2`
- `pub const MAX_WORKERS: usize = 8`
- `pub const QUEUE_PER_WORKER: usize = 64`
- `pub const WORKER_MEMORY_BUDGET: u64 = 16 * 1024 * 1024`
- `pub const MEMORY_DIVISOR: u64 = 24`
- `pub const MIN_MEMORY_BUDGET: u64 = 64 * 1024 * 1024`
- `pub const MAX_MEMORY_BUDGET: u64 = 512 * 1024 * 1024`
- `pub const DEFAULT_TASK_TIMEOUT_SECS: u64 = 30`
- `pub const FALLBACK_MEMORY_BYTES: u64 = 4 * 1024 * 1024 * 1024`
- `pub struct RendererPlan`
- `impl RendererPlan`
  - `pub fn threads(&self) -> usize`
  - `pub fn queue_capacity(&self) -> usize`
  - `pub fn memory_budget(&self) -> u64`
- `pub fn reserved_cpus(cpus: usize) -> usize`
- `pub fn detect_workers(cpus: usize) -> usize`
- `pub fn detect_memory_budget(mem_bytes: u64) -> u64`
- `pub fn detect_affordable_workers(mem_bytes: u64) -> usize`
- `pub fn detect_worker_count(cpus: usize, mem_bytes: u64) -> usize`
- `pub fn detect_queue_capacity(workers: usize) -> usize`
- `pub fn detect_pool_config(cpus: usize, mem_bytes: u64) -> RenderPoolConfig`
- `pub fn detect_scheduler_config(cpus: usize, mem_bytes: u64) -> SchedulerConfig`
- `pub fn detect_plan(cpus: usize, mem_bytes: u64) -> RendererPlan`
- `pub fn probe_cpus() -> usize`
- `pub fn probe_memory_bytes() -> u64`
- `pub fn parse_meminfo_total(meminfo: &str) -> Option<u64>`
- `pub fn current_plan() -> RendererPlan`
- `pub fn surface_cache_budget() -> u64`

## `src/pool.rs`

- `pub type PoolJob = Box<dyn FnOnce() -> Result<Bytes, RenderError> + Send + 'static>`
- `pub struct RenderPoolConfig`
- `impl RenderPoolConfig`
  - `pub fn new(threads: usize) -> Self`
  - `pub fn with_threads(mut self, threads: usize) -> Self`
  - `pub fn with_queue_capacity(mut self, queue_capacity: usize) -> Self`
- `pub struct PoolStats`
- `pub struct SurfaceReuseStats`
- `impl SurfaceReuseStats`
  - `pub fn total(&self) -> u64`
  - `pub fn hit_rate(&self) -> f32`
- `pub fn surface_reuse_stats() -> SurfaceReuseStats`
- `pub fn reset_surface_reuse_stats()`
- `impl BufferSlot`
- `impl SurfaceCache`
- `pub fn thread_reuse_stats() -> SurfaceReuseStats`
- `pub fn is_pool_worker() -> bool`
- `pub fn with_surface<R>( …`
- `pub fn paint<D, F, R>(width: i32, height: i32, draw: D, finish: F) -> Result<R, RenderError>`
- `pub struct RenderPool`
- `impl RenderPool`
  - `pub fn new() -> Result<Arc<Self>, RenderError>`
  - `pub fn with_config(config: RenderPoolConfig) -> Result<Arc<Self>, RenderError>`
  - `pub fn config(&self) -> RenderPoolConfig`
  - `pub fn thread_count(&self) -> usize`
  - `pub fn live_threads(&self) -> usize`
  - `pub fn is_closed(&self) -> bool`
  - `pub fn queue_len(&self) -> usize`
  - `pub fn submit( …`
  - `pub fn execute( …`
  - `pub fn stats(&self) -> PoolStats`
  - `pub fn surface_reuse_stats(&self) -> SurfaceReuseStats`
  - `pub fn shutdown(&self)`
- `pub struct PoolTicket`
- `impl PoolTicket`
  - `pub fn wait_blocking(self) -> Result<Bytes, RenderError>`
  - `pub fn receiver(self) -> oneshot::Receiver<Result<Bytes, RenderError>>`

## `src/scheduler.rs`

- `pub struct SchedulerConfig`
- `impl SchedulerConfig`
  - `pub fn texture_capacity(&self) -> u64`
- `pub struct SchedulerMetrics`
- `impl Inner`
- `pub struct RenderScheduler`
- `impl RenderScheduler`
  - `pub fn new(config: SchedulerConfig, backend: BackendHandle) -> Arc<Self>`
  - `pub fn try_new( …`
  - `pub fn metrics(&self) -> SchedulerMetrics`
  - `pub fn backend(&self) -> &BackendHandle`
  - `pub fn context(&self) -> &Arc<RenderContext>`
  - `pub fn pool(&self) -> &Arc<RenderPool>`
  - `pub fn config(&self) -> &SchedulerConfig`
  - `pub fn is_closed(&self) -> bool`
- `pub struct RenderHandle`
- `impl RenderHandle`
  - `pub fn ticket(&self) -> u64`
  - `pub fn ticket_slot(&self) -> RenderTicket`

## `src/task.rs`

- `pub enum RenderPriority`
- `pub struct RenderTask`
- `impl RenderTask`
  - `pub fn new(name: impl Into<String>) -> Self`
  - `pub fn with_priority(mut self, priority: RenderPriority) -> Self`
  - `pub fn with_estimated_bytes(mut self, estimated_bytes: u64) -> Self`
  - `pub fn name(&self) -> &str`
- `pub struct RenderTicket`
- `impl RenderTicket`
  - `pub fn new(id: u64) -> Self`
  - `pub fn id(&self) -> u64`
- `pub struct RenderOutput`
- `pub type RenderJob = Box<dyn FnOnce(&RenderContext) -> Result<Bytes, RenderError> + Send>`

