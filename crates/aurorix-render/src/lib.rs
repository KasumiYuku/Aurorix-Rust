pub mod asset;
pub mod backend;
pub mod batch;
pub mod cache;
pub mod card_cache;
pub mod codec;
pub mod context;
pub mod draw;
pub mod error;
pub mod fonts;
pub mod gpu;
pub mod handle;
pub mod kit;
pub mod layer;
pub mod memory;
pub mod plan;
pub mod pool;
pub mod scheduler;
pub mod settings;
pub mod task;

pub use asset::{AssetLoader, ChainedAssets};
pub use backend::{
    BackendCapabilities, BackendHandle, PlaceholderBackend, RenderBackend, SkiaBackend,
};
pub use batch::{PlacedText, Sprite, TextBlobCache, TextBlobKey, TextBlobStats};
pub use cache::{content_key, CacheStats, Texture, TextureCache};
pub use codec::{
    default_webp_method, encode_rgba, encode_rgba_image, encode_surface, EncodeOptions,
    RenderQuality, RenderedImage,
};
pub use context::{FontBook, FontHandle, RenderContext};
pub use draw::{
    crisp_font, draw_text, fill, fill_rect, measure_text, text_paint, Color, FontWeight,
    RenderFormat, RenderRequest, TextAlign, TextStyle,
};
pub use error::{RenderError, RenderResult};
pub use fonts::{
    clear_font_families, collection_alias, collection_face_count, collection_face_index,
    collection_header, data_font_manager, font_container, font_face, font_family_bytes,
    font_family_count, font_family_names, has_font_family, normalize_family,
    register_font_collection, register_font_family, FontContainer, FontFace, COLLECTION_FACE_LIMIT,
};
pub use gpu::{
    decode_version, fallback_count, hardware_accelerated, note_failure, probe, probe_with,
    reset_failure, runtime_state, GpuDeviceKind, GpuInfo, GpuMode, GpuRuntimeState, GpuSupport,
    GPU_ENV, GPU_FEATURE_ENABLED,
};
pub use handle::{SharedFontMgr, SharedImage, SharedTypeface};
pub use kit::{
    contain_destination_rect, cover_source_rect, default_sampling, sampling_crisp, sampling_cubic,
    sampling_for, sampling_linear, sampling_nearest, solid_paint, Border, CanvasKit, FitMode, FontChain, Gradient,
    GradientStop, ImageSource, LinearGradient, Padding, RadialGradient, TextLayout, TextMetrics,
    TEXT_BLOB_MAX_BYTES,
};
pub use layer::RecordedLayer;
pub use memory::{
    after_render, before_render, emergency_reclaim, pressure, reclaim, ReclaimReport,
};
pub use plan::{
    current_plan, detect_plan, detect_pool_config, detect_scheduler_config, detect_workers,
    surface_cache_budget, RendererPlan,
};
pub use pool::{
    is_pool_worker, reset_surface_reuse_stats, surface_reuse_stats, thread_reuse_stats, PoolJob,
    PoolStats, PoolTicket, RenderPool, RenderPoolConfig, SurfaceReuseStats,
};
pub use scheduler::{RenderHandle, RenderScheduler, SchedulerConfig, SchedulerMetrics};
pub use settings::{
    apply_settings, current_settings, effective_concurrency, effective_format, effective_queue, render_budget_bytes,
    render_budget_mb, render_concurrency, render_queue, set_render_budget_mb,
    set_render_concurrency, set_render_queue, set_webp_enabled, webp_enabled,
    RenderSettings,
};
pub use skia_safe;
pub use task::{RenderJob, RenderOutput, RenderPriority, RenderTask, RenderTicket};
pub use webp;
