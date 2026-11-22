use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use aurorix_core::MemoryPressure;
use bytes::Bytes;
use dashmap::DashMap;
use parking_lot::Mutex;
use skia_safe::Canvas;

use crate::asset::AssetLoader;
use crate::backend::BackendHandle;
use crate::batch::TextBlobCache;
use crate::cache::{Texture, TextureCache};
use crate::codec::{self, EncodeOptions, RenderedImage};
use crate::draw::{self, Color, RenderRequest, TextStyle};
use crate::error::RenderError;
use crate::fonts;
use crate::handle::{SharedFontMgr, SharedTypeface};
use crate::memory::ReclaimReport;
use crate::pool;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontHandle {
    pub family: Arc<str>,
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FontKey {
    family: Arc<str>,
    size: u32,
}

impl FontKey {
    fn new(family: &str, size: u32) -> Self {
        Self {
            family: Arc::from(family),
            size,
        }
    }
}

struct FontSlot {
    key: FontKey,
    handle: FontHandle,
    loaded: Mutex<bool>,
}

impl FontSlot {
    fn new(key: FontKey) -> Self {
        Self {
            handle: FontHandle {
                family: Arc::clone(&key.family),
                size: key.size,
            },
            key,
            loaded: Mutex::new(false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TypefaceKey {
    family: Arc<str>,
    bold: bool,
}

impl TypefaceKey {
    fn new(family: &str, bold: bool) -> Self {
        Self {
            family: Arc::from(family),
            bold,
        }
    }
}

struct TypefaceSlot {
    resolved: Mutex<Option<Arc<SharedTypeface>>>,
}

impl TypefaceSlot {
    fn new() -> Self {
        Self {
            resolved: Mutex::new(None),
        }
    }
}

pub struct FontBook {
    capacity: usize,
    manager: Option<Arc<SharedFontMgr>>,
    slots: DashMap<FontKey, Arc<FontSlot>>,
    order: Mutex<VecDeque<FontKey>>,
    typefaces: DashMap<TypefaceKey, Arc<TypefaceSlot>>,
}

impl FontBook {
    pub fn new(capacity: usize, manager: Option<Arc<SharedFontMgr>>) -> Self {
        Self {
            capacity: capacity.max(1),
            manager,
            slots: DashMap::new(),
            order: Mutex::new(VecDeque::new()),
            typefaces: DashMap::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn manager(&self) -> Option<&Arc<SharedFontMgr>> {
        self.manager.as_ref()
    }

    pub fn family_count(&self) -> usize {
        self.manager
            .as_ref()
            .map(|manager| manager.family_count())
            .unwrap_or(0)
    }

    pub fn get(&self, family: &str, size: u32) -> Option<FontHandle> {
        let key = FontKey::new(family, size);
        let slot = self.slots.get(&key)?;
        let loaded = *slot.loaded.lock();
        let handle = slot.handle.clone();
        drop(slot);
        if loaded {
            Some(handle)
        } else {
            None
        }
    }

    pub fn load(
        &self,
        family: &str,
        size: u32,
        loader: impl FnOnce(&str, u32) -> Result<Arc<Vec<u8>>, RenderError>,
    ) -> Result<FontHandle, RenderError> {
        let key = FontKey::new(family, size);
        let slot = match self.slots.get(&key) {
            Some(existing) => Arc::clone(&*existing),
            None => {
                let fresh = Arc::new(FontSlot::new(key.clone()));
                let entry = self
                    .slots
                    .entry(key.clone())
                    .or_insert_with(|| Arc::clone(&fresh));
                let slot = Arc::clone(&*entry);
                drop(entry);
                if Arc::ptr_eq(&slot, &fresh) {
                    self.track(key.clone());
                }
                slot
            }
        };
        if let Some(bytes) = load_bytes(&slot, loader)? {
            self.warm_typeface(&slot.key.family, false, &bytes);
        }
        Ok(slot.handle.clone())
    }

    pub fn typeface(&self, family: &str, bold: bool) -> Result<Arc<SharedTypeface>, RenderError> {
        let key = TypefaceKey::new(family, bold);
        let slot = match self.typefaces.get(&key) {
            Some(existing) => Arc::clone(&*existing),
            None => {
                let fresh = Arc::new(TypefaceSlot::new());
                let entry = self
                    .typefaces
                    .entry(key.clone())
                    .or_insert_with(|| Arc::clone(&fresh));
                Arc::clone(&*entry)
            }
        };
        let mut resolved = slot.resolved.lock();
        if let Some(cached) = resolved.as_ref() {
            return Ok(Arc::clone(cached));
        }
        if let Some(bytes) = fonts::font_family_bytes(family, bold) {
            if let Some(registered) = self.typeface_from_bytes(family, &bytes) {
                let shared = Arc::new(registered);
                *resolved = Some(Arc::clone(&shared));
                return Ok(shared);
            }
        }
        let manager = self.manager.as_ref().ok_or_else(|| {
            RenderError::Font(format!(
                "后端未提供字体管理器, 也找不到已注册字体族 {family}"
            ))
        })?;
        let found = manager
            .match_family(family, bold)
            .or_else(|| manager.default_typeface(bold))
            .ok_or_else(|| RenderError::Font(format!("字体管理器里没有可用字形: {family}")))?;
        let shared = Arc::new(found);
        *resolved = Some(Arc::clone(&shared));
        Ok(shared)
    }

    pub fn registered_families(&self) -> Vec<String> {
        fonts::font_family_names()
    }

    pub fn registered_family_count(&self) -> usize {
        fonts::font_family_count()
    }

    pub fn has_registered_family(&self, family: &str) -> bool {
        fonts::has_font_family(family)
    }

    fn typeface_from_bytes(&self, family: &str, data: &[u8]) -> Option<SharedTypeface> {
        let index = fonts::collection_face_index(family);
        match self.manager.as_ref() {
            Some(manager) => manager.typeface_at(data, index),
            None => fonts::data_font_manager().typeface_at(data, index),
        }
    }

    pub fn fallback_chain(&self, family: &str, bold: bool) -> Vec<Arc<SharedTypeface>> {
        let mut chain = Vec::new();
        if let Ok(matched) = self.typeface(family, bold) {
            chain.push(matched);
        }
        if let Some(default) = self
            .manager
            .as_ref()
            .and_then(|manager| manager.default_typeface(bold))
        {
            let default = Arc::new(default);
            if !chain
                .iter()
                .any(|existing| existing.family_name() == default.family_name())
            {
                chain.push(default);
            }
        }
        chain
    }

    fn warm_typeface(&self, family: &str, bold: bool, data: &[u8]) {
        let Some(manager) = self.manager.as_ref() else {
            return;
        };
        let key = TypefaceKey::new(family, bold);
        if self.typefaces.contains_key(&key) {
            return;
        }
        let resolved = manager
            .typeface_at(data, fonts::collection_face_index(family))
            .or_else(|| manager.match_family(family, bold))
            .or_else(|| manager.default_typeface(bold));
        if let Some(typeface) = resolved {
            let slot = Arc::new(TypefaceSlot::new());
            *slot.resolved.lock() = Some(Arc::new(typeface));
            self.typefaces.insert(key, slot);
        }
    }

    fn track(&self, key: FontKey) {
        let mut order = self.order.lock();
        order.push_back(key);
        while order.len() > self.capacity {
            let Some(evicted) = order.pop_front() else {
                break;
            };
            self.slots.remove(&evicted);
        }
    }
}

fn load_bytes(
    slot: &Arc<FontSlot>,
    loader: impl FnOnce(&str, u32) -> Result<Arc<Vec<u8>>, RenderError>,
) -> Result<Option<Arc<Vec<u8>>>, RenderError> {
    let mut loaded = slot.loaded.lock();
    if *loaded {
        return Ok(None);
    }
    let bytes = loader(&slot.key.family, slot.key.size)?;
    *loaded = true;
    Ok(Some(bytes))
}

pub struct RenderContext {
    backend: BackendHandle,
    fonts: Arc<FontBook>,
    textures: Arc<TextureCache>,
    text_blobs: Arc<TextBlobCache>,
    layouts: Arc<DashMap<LayoutKey, Arc<crate::kit::TextLayout>>>,
    assets: Option<Arc<dyn AssetLoader>>,
}

pub type LayoutKey = (String, u32, bool);

impl RenderContext {
    pub fn new(backend: BackendHandle, font_capacity: usize, texture_capacity: u64) -> Arc<Self> {
        let fonts = FontBook::new(font_capacity, backend.font_manager());
        let textures = Arc::new(TextureCache::new(texture_capacity));
        crate::memory::register_texture_cache(&textures);
        let text_blobs = Arc::new(TextBlobCache::new(crate::memory::text_blob_capacity()));
        crate::memory::register_text_blob_cache(&text_blobs);
        Arc::new(Self {
            backend,
            fonts: Arc::new(fonts),
            textures,
            text_blobs,
            layouts: Arc::new(DashMap::new()),
            assets: None,
        })
    }

    pub fn pressure(&self) -> MemoryPressure {
        crate::memory::pressure()
    }

    pub fn emergency_reclaim(&self) -> ReclaimReport {
        crate::memory::emergency_reclaim()
    }

    pub fn backend(&self) -> &BackendHandle {
        &self.backend
    }

    pub fn fonts(&self) -> &FontBook {
        &self.fonts
    }

    pub fn textures(&self) -> &TextureCache {
        &self.textures
    }

    pub fn text_blobs(&self) -> &TextBlobCache {
        &self.text_blobs
    }

    pub fn layouts(&self) -> &DashMap<LayoutKey, Arc<crate::kit::TextLayout>> {
        &self.layouts
    }
    pub fn draw<F>(&self, request: RenderRequest, painter: F) -> Result<Bytes, RenderError>
    where
        F: FnOnce(&Canvas, &RenderContext),
    {
        Ok(self.draw_image(request, painter)?.bytes)
    }

    pub fn draw_image<F>(
        &self,
        request: RenderRequest,
        painter: F,
    ) -> Result<RenderedImage, RenderError>
    where
        F: FnOnce(&Canvas, &RenderContext),
    {
        let options = EncodeOptions::from_request(&request);
        self.draw_with_options_image(request, options, painter)
    }

    pub fn with_asset_loader(self: &Arc<Self>, loader: Arc<dyn AssetLoader>) -> Arc<Self> {
        Arc::new(Self {
            backend: Arc::clone(&self.backend),
            fonts: Arc::clone(&self.fonts),
            textures: Arc::clone(&self.textures),
            text_blobs: Arc::clone(&self.text_blobs),
            layouts: Arc::clone(&self.layouts),
            assets: Some(loader),
        })
    }

    pub fn assets(&self) -> Option<&Arc<dyn AssetLoader>> {
        self.assets.as_ref()
    }

    pub fn has_asset_loader(&self) -> bool {
        self.assets.is_some()
    }

    pub fn asset(&self, key: &str) -> Result<Arc<Texture>, RenderError> {
        if let Some(hit) = self.textures.get(key) {
            if hit.image().is_some() {
                return Ok(hit);
            }
            self.textures.remove(key);
        }
        let loader = self.assets.as_ref().ok_or_else(|| {
            RenderError::Texture(format!("渲染上下文没有配置素材加载器, 无法读取 {key}"))
        })?;
        let bytes = loader.load(key)?;
        self.textures.insert_encoded(key, bytes)
    }

    pub fn draw_with_options<F>(
        &self,
        request: RenderRequest,
        options: EncodeOptions,
        painter: F,
    ) -> Result<Bytes, RenderError>
    where
        F: FnOnce(&Canvas, &RenderContext),
    {
        Ok(self
            .draw_with_options_image(request, options, painter)?
            .bytes)
    }

    pub fn draw_with_options_image<F>(
        &self,
        request: RenderRequest,
        options: EncodeOptions,
        painter: F,
    ) -> Result<RenderedImage, RenderError>
    where
        F: FnOnce(&Canvas, &RenderContext),
    {
        if request.width <= 0 || request.height <= 0 {
            return Err(RenderError::Backend(format!(
                "画布尺寸非法: {}x{}",
                request.width, request.height
            )));
        }
        let (physical_width, physical_height) = request.physical_size();
        if physical_width <= 0 || physical_height <= 0 {
            return Err(RenderError::Backend(format!(
                "物理画布尺寸非法: {physical_width}x{physical_height} (逻辑 {}x{}, 倍率 {})",
                request.width,
                request.height,
                request.effective_scale()
            )));
        }
        let scale = request.effective_scale();
        let background = request.effective_background();
        let format = crate::settings::effective_format(request.format);
        let profile = std::env::var_os("RENDER_PROFILE").is_some();
        let painted = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let started = std::time::Instant::now();
        let painted_in_painter = std::sync::Arc::clone(&painted);
        let painted_in_encode = std::sync::Arc::clone(&painted);
        let result = pool::paint(
            physical_width,
            physical_height,
            |surface| {
                let canvas = surface.canvas();
                canvas.clear(Color::TRANSPARENT.to_skia());
                if scale != 1.0 {
                    canvas.scale((scale, scale));
                }
                if let Some(background) = background {
                    draw::fill(canvas, background);
                }
                painter(canvas, self);
                painted_in_painter.store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
                Ok(())
            },
            |pixels, row_bytes| {
                let encode_started = std::time::Instant::now();
                let encoded = codec::encode_rgba_image_mut(
                    format,
                    pixels,
                    physical_width,
                    physical_height,
                    row_bytes,
                    &options,
                );
                if profile {
                    eprintln!(
                        "profile {}x{} paint {}ms encode {}ms",
                        physical_width,
                        physical_height,
                        painted_in_encode.load(Ordering::Relaxed) / 1000,
                        encode_started.elapsed().as_millis()
                    );
                }
                encoded
            },
        );
        result
    }

    pub fn draw_text(
        &self,
        canvas: &Canvas,
        text: &str,
        x: f32,
        y: f32,
        style: &TextStyle,
    ) -> Result<f32, RenderError> {
        draw::draw_text(canvas, &self.fonts, text, x, y, style)
    }

    pub fn measure_text(&self, text: &str, style: &TextStyle) -> Result<f32, RenderError> {
        draw::measure_text(&self.fonts, text, style)
    }
}
