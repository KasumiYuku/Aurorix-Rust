use std::collections::BTreeMap;
use std::sync::Arc;

use aurorix_render::skia_safe::{surfaces, EncodedImageFormat, Rect};
use aurorix_render::{
    AssetLoader, CanvasKit, RenderContext, RenderError, SkiaBackend, TextureCache,
};
use bytes::Bytes;

struct MapLoader {
    entries: BTreeMap<String, Bytes>,
}

impl MapLoader {
    fn new(entries: [(&str, Bytes); 1]) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        }
    }
}

impl AssetLoader for MapLoader {
    fn load(&self, key: &str) -> Result<Bytes, RenderError> {
        self.entries
            .get(key)
            .cloned()
            .ok_or_else(|| RenderError::Texture(format!("内存素材里没有 {key}")))
    }
}

fn png(width: i32, height: i32) -> Bytes {
    let mut surface = surfaces::raster_n32_premul((width, height)).expect("画布");
    surface
        .canvas()
        .clear(aurorix_render::skia_safe::Color::from_argb(
            255, 236, 62, 140,
        ));
    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, 100)
        .expect("png 编码");
    Bytes::copy_from_slice(data.as_bytes())
}

fn library() -> (Arc<RenderContext>, Bytes) {
    let bytes = png(64, 32);
    let base = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 4 * 1024 * 1024);
    let loader = Arc::new(MapLoader::new([("covers/hero", bytes.clone())]));
    (base.with_asset_loader(loader), bytes)
}

#[test]
fn asset_is_loaded_once_and_reused_from_the_lru() {
    let (context, bytes) = library();
    assert!(context.has_asset_loader());
    assert!(context.assets().is_some());
    let mut surface = surfaces::raster_n32_premul((16, 16)).expect("画布");
    let kit = CanvasKit::new(surface.canvas(), &context);
    let first = kit.asset("covers/hero").expect("第一次读取");
    let second = kit.asset("covers/hero").expect("第二次读取");
    assert!(Arc::ptr_eq(&first, &second), "命中缓存要复用同一个 Arc");
    assert_eq!(first.len(), bytes.len());
    assert_eq!(first.dimensions(), Some((64, 32)));
    assert!(first.image().is_some(), "素材应当被解码成位图");
    let stats = kit.textures().stats();
    assert_eq!(stats.entries, 1, "缓存里只应有一个条目");
    assert_eq!(stats.misses, 1, "只应发生一次未命中");
    assert_eq!(stats.hits, 1, "第二次读取要命中");
    assert_eq!(stats.bytes, bytes.len() as u64);
    assert_eq!(kit.textures().capacity_bytes(), 4 * 1024 * 1024);
}

#[test]
fn missing_key_and_missing_loader_are_errors() {
    let (context, _) = library();
    let mut surface = surfaces::raster_n32_premul((8, 8)).expect("画布");
    let kit = CanvasKit::new(surface.canvas(), &context);
    let error = kit.asset("covers/none").expect_err("不存在的素材必须报错");
    assert!(matches!(error, RenderError::Texture(_)), "{error:?}");
    let bare = RenderContext::new(Arc::new(SkiaBackend::new()), 4, 1 << 20);
    assert!(!bare.has_asset_loader());
    assert!(bare.assets().is_none());
    let mut bare_surface = surfaces::raster_n32_premul((8, 8)).expect("画布");
    let bare_kit = CanvasKit::new(bare_surface.canvas(), &bare);
    let error = bare_kit.asset("covers/hero").expect_err("没有加载器要报错");
    assert!(matches!(error, RenderError::Texture(_)), "{error:?}");
}

#[test]
fn over_capacity_texture_reports_the_budget() {
    let bytes = png(256, 256);
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 4, 16);
    let loader = Arc::new(MapLoader::new([("big", bytes)]));
    let context = context.with_asset_loader(loader);
    let mut surface = surfaces::raster_n32_premul((8, 8)).expect("画布");
    let kit = CanvasKit::new(surface.canvas(), &context);
    let error = kit.asset("big").expect_err("超出预算必须报错");
    assert!(matches!(error, RenderError::Texture(_)), "{error:?}");
    assert_eq!(kit.textures().stats().entries, 0);
}

#[test]
fn eviction_keeps_the_cache_within_budget() {
    let cache = TextureCache::new(png(8, 8).len() as u64 * 2 + 16);
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 4, cache.capacity_bytes());
    let loader = Arc::new(MapLoader::new([("one", png(8, 8))]));
    let context = context.with_asset_loader(loader);
    let mut surface = surfaces::raster_n32_premul((8, 8)).expect("画布");
    let kit = CanvasKit::new(surface.canvas(), &context);
    kit.asset("one").expect("第一个素材");
    let stored = |kit: &CanvasKit<'_>| kit.textures().stats();
    assert_eq!(stored(&kit).entries, 1);
    let loader = Arc::new(MapLoader::new([("two", png(8, 8))]));
    let context = context.with_asset_loader(loader);
    let fresh = CanvasKit::new(surface.canvas(), &context);
    fresh.asset("two").expect("第二个素材");
    assert!(fresh.textures().stats().entries <= 2);
    assert!(fresh.textures().stats().bytes <= fresh.textures().capacity_bytes());
}

#[test]
fn draw_asset_and_cover_paint_pixels_inside_the_clip() {
    let (context, _) = library();
    let mut surface = surfaces::raster_n32_premul((120, 120)).expect("画布");
    {
        let kit = CanvasKit::new(surface.canvas(), &context);
        kit.draw_asset("covers/hero", Rect::from_xywh(4.0, 4.0, 48.0, 48.0), 8.0)
            .expect("draw_asset");
        kit.draw_asset_cover("covers/hero", Rect::from_xywh(64.0, 4.0, 32.0, 64.0), 10.0)
            .expect("draw_asset_cover");
    }
    let image = surface.image_snapshot();
    let pixmap = image.peek_pixels().expect("像素可读");
    let pixels = pixmap.bytes().expect("像素字节");
    let row = pixmap.row_bytes();
    let pixel = |x: usize, y: usize| {
        let offset = y * row + x * 4;
        (
            pixels[offset],
            pixels[offset + 1],
            pixels[offset + 2],
            pixels[offset + 3],
        )
    };
    let contained = pixel(28, 28);
    assert!(contained.3 > 200, "圆角内应当有素材像素: {contained:?}");
    let spread = (contained.0 as i32 - contained.2 as i32).abs();
    assert!(
        contained.0.max(contained.2) > 180 && spread > 40,
        "素材颜色要保留: {contained:?}"
    );
    let covered = pixel(80, 40);
    assert!(covered.3 > 200, "cover 区域应当被填满: {covered:?}");
    let outside = pixel(2, 2);
    assert_eq!(outside.3, 0, "矩形之外必须透明: {outside:?}");
    let corner = pixel(4, 4);
    assert!(corner.3 < 200, "圆角以外应当被裁掉: {corner:?}");
}

#[test]
fn contain_destination_rect_keeps_the_aspect_ratio() {
    let mut surface = surfaces::raster_n32_premul((80, 40)).expect("画布");
    let image = surface.image_snapshot();
    let target = Rect::from_xywh(0.0, 0.0, 40.0, 40.0);
    let fitted = aurorix_render::contain_destination_rect(&image, target);
    assert!((fitted.width() - 40.0).abs() < 0.01, "{fitted:?}");
    assert!((fitted.height() - 20.0).abs() < 0.01, "{fitted:?}");
    assert!((fitted.top() - 10.0).abs() < 0.01, "{fitted:?}");
    let wide = Rect::from_xywh(0.0, 0.0, 160.0, 40.0);
    let fitted = aurorix_render::contain_destination_rect(&image, wide);
    assert!((fitted.width() - 80.0).abs() < 0.01, "{fitted:?}");
    assert!((fitted.left() - 40.0).abs() < 0.01, "{fitted:?}");
}
