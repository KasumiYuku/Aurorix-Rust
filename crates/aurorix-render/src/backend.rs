use std::sync::Arc;
use std::thread;

use crate::handle::SharedFontMgr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub max_texture_side: u32,
    pub max_texture_bytes: u64,
    pub hardware_accelerated: bool,
    pub concurrent_contexts: usize,
}

pub trait RenderBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> BackendCapabilities;
    fn font_manager(&self) -> Option<Arc<SharedFontMgr>> {
        None
    }
}

pub type BackendHandle = Arc<dyn RenderBackend>;

const PLACEHOLDER_CAPABILITIES: BackendCapabilities = BackendCapabilities {
    max_texture_side: 4096,
    max_texture_bytes: 64 * 1024 * 1024,
    hardware_accelerated: false,
    concurrent_contexts: 4,
};

const SKIA_MAX_TEXTURE_SIDE: u32 = 16384;
const SKIA_MAX_TEXTURE_BYTES: u64 = 512 * 1024 * 1024;
const SKIA_DEFAULT_CONCURRENT_CONTEXTS: usize = 4;
const SKIA_MAX_CONCURRENT_CONTEXTS: usize = 8;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PlaceholderBackend;

impl PlaceholderBackend {
    pub fn new() -> Self {
        Self
    }

    pub fn handle() -> BackendHandle {
        Arc::new(Self)
    }
}

impl RenderBackend for PlaceholderBackend {
    fn name(&self) -> &'static str {
        "placeholder-cpu"
    }

    fn capabilities(&self) -> BackendCapabilities {
        PLACEHOLDER_CAPABILITIES
    }
}

pub struct SkiaBackend {
    fonts: Arc<SharedFontMgr>,
    capabilities: BackendCapabilities,
}

impl SkiaBackend {
    pub fn new() -> Self {
        Self::with_font_manager(Arc::new(SharedFontMgr::system()))
    }

    pub fn with_font_manager(fonts: Arc<SharedFontMgr>) -> Self {
        let capabilities = detect_capabilities();
        Self {
            fonts,
            capabilities,
        }
    }

    pub fn handle() -> BackendHandle {
        Arc::new(Self::new())
    }

    pub fn font_manager(&self) -> Arc<SharedFontMgr> {
        Arc::clone(&self.fonts)
    }

    pub fn font_family_count(&self) -> usize {
        self.fonts.family_count()
    }
}

impl Default for SkiaBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SkiaBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SkiaBackend")
            .field("families", &self.fonts.family_count())
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

impl RenderBackend for SkiaBackend {
    fn name(&self) -> &'static str {
        "skia-safe"
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.capabilities
    }

    fn font_manager(&self) -> Option<Arc<SharedFontMgr>> {
        Some(Arc::clone(&self.fonts))
    }
}

fn detect_capabilities() -> BackendCapabilities {
    let parallel = thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(SKIA_DEFAULT_CONCURRENT_CONTEXTS);
    BackendCapabilities {
        max_texture_side: SKIA_MAX_TEXTURE_SIDE,
        max_texture_bytes: SKIA_MAX_TEXTURE_BYTES,
        hardware_accelerated: crate::gpu::hardware_accelerated(),
        concurrent_contexts: parallel.clamp(1, SKIA_MAX_CONCURRENT_CONTEXTS),
    }
}
