use std::path::{Path, PathBuf};
use std::sync::Arc;

use aurorix_plugin::PluginAssets;
use aurorix_render::skia_safe::{surfaces, Color as SkColor, EncodedImageFormat, Rect};
use aurorix_render::{CanvasKit, PlaceholderBackend, RenderContext, SkiaBackend, TextStyle};
use aurorix_template::{Registry, TemplateKind};
use example_rating::{
    demo_scores, draw_card, draw_card_with_assets, row_height, row_rect, Score, CARD_HEIGHT,
    CARD_WIDTH, COVER_ASSET_KEY,
};

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "aurorix-rating-assets-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("创建临时目录");
        Self { root }
    }

    fn file(&self, relative: &str, data: &[u8]) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("创建父目录");
        }
        std::fs::write(&path, data).expect("写文件");
        path
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn collect_fonts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, depth + 1, out);
        } else if path
            .extension()
            .map(|extension| extension.eq_ignore_ascii_case("ttf"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

fn system_font() -> Option<(String, Vec<u8>)> {
    let roots = [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/System/Library/Fonts",
        "C:/Windows/Fonts",
    ];
    let mut fonts = Vec::new();
    for root in roots {
        collect_fonts(Path::new(root), 0, &mut fonts);
    }
    fonts.sort();
    fonts.iter().find_map(|path| {
        let stem = path.file_stem()?.to_string_lossy().to_string();
        let bytes = std::fs::read(path).ok()?;
        Some((stem, bytes))
    })
}

fn pink_png() -> Vec<u8> {
    let mut surface = surfaces::raster_n32_premul((96, 96)).expect("画布");
    surface
        .canvas()
        .clear(SkColor::from_argb(255, 246, 64, 148));
    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, 100)
        .expect("png 编码");
    data.as_bytes().to_vec()
}

fn scores() -> Vec<Score> {
    demo_scores().into_iter().take(3).collect()
}

fn average(scores: &[Score]) -> f64 {
    scores.iter().map(|score| score.rating).sum::<f64>() / scores.len() as f64
}

#[test]
fn bootstrap_registers_plugin_fonts_and_templates() {
    let tree = TempTree::new("bootstrap");
    tree.file(COVER_ASSET_KEY, &pink_png());
    tree.file("templates/card.md", b"card {{v}}");
    let font = system_font();
    if let Some((stem, bytes)) = font.as_ref() {
        tree.file(&format!("fonts/{stem}.ttf"), bytes);
    } else {
        println!("本机没有可用 ttf 字体, 跳过字体注册断言");
    }
    let assets = PluginAssets::new("rating", &tree.root);
    assert_eq!(assets.plugin_id(), "rating");
    assert!(assets.exists(COVER_ASSET_KEY));
    let registry = Arc::new(Registry::new());
    let report = assets.bootstrap(&registry);
    assert!(report.root_ready);
    assert_eq!(report.templates, vec!["card".to_string()]);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    if let Some((stem, _)) = font.as_ref() {
        assert_eq!(report.fonts, vec![stem.clone()]);
        assert_eq!(assets.font_alias(stem), Some(stem.clone()));
        assert!(aurorix_render::has_font_family(stem));
    }
    let args = aurorix_template::args_map! { "v" => "X" };
    assert_eq!(
        registry
            .fill(TemplateKind::Markdown, "rating", "card", &args)
            .expect("插件命名空间模板"),
        "card X"
    );
}

#[test]
fn card_uses_plugin_asset_for_the_first_row_cover() {
    let tree = TempTree::new("cover");
    tree.file(COVER_ASSET_KEY, &pink_png());
    let assets = Arc::new(PluginAssets::new("rating", &tree.root));
    let base = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 32 * 1024 * 1024);
    let context = base.with_asset_loader(assets.loader());
    let scores = scores();
    let average = average(&scores);
    let mut surface = surfaces::raster_n32_premul((CARD_WIDTH, CARD_HEIGHT)).expect("画布");
    {
        let kit = CanvasKit::new(surface.canvas(), &context);
        draw_card_with_assets(&kit, "alice", &scores, average, Some(COVER_ASSET_KEY), None);
    }
    let stats = context.textures().stats();
    assert_eq!(stats.entries, 1, "素材应当进入纹理缓存");
    assert_eq!(stats.misses, 1);
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
    let rect = row_rect(0, scores.len());
    assert!(row_height(scores.len()) > 40.0);
    let probe = pixel((rect.left() + 30.0) as usize, rect.center_y() as usize);
    assert_eq!(probe.3, 255, "素材区域应当不透明: {probe:?}");
    let spread = (probe.0 as i32 - probe.1 as i32).abs() + (probe.1 as i32 - probe.2 as i32).abs();
    assert!(spread > 60, "第一行封面应当来自粉色素材: {probe:?}");

    let mut second = surfaces::raster_n32_premul((CARD_WIDTH, CARD_HEIGHT)).expect("画布");
    {
        let kit = CanvasKit::new(second.canvas(), &context);
        let _ = kit.draw_asset_cover(COVER_ASSET_KEY, Rect::from_xywh(0.0, 0.0, 32.0, 32.0), 8.0);
    }
    assert!(
        context.textures().stats().hits >= 1,
        "第二次使用应当命中缓存"
    );
}

#[test]
fn registered_plugin_font_draws_the_header() {
    let Some((stem, bytes)) = system_font() else {
        println!("本机没有可用 ttf 字体, 跳过插件字体断言");
        return;
    };
    let tree = TempTree::new("font");
    tree.file(&format!("fonts/{stem}.ttf"), &bytes);
    let assets = Arc::new(PluginAssets::new("rating", &tree.root));
    let registry = Arc::new(Registry::new());
    let report = assets.bootstrap(&registry);
    assert_eq!(report.fonts, vec![stem.clone()]);
    let alias = assets.font_alias(&stem).expect("别名");
    let context = RenderContext::new(Arc::new(PlaceholderBackend::new()), 8, 8 * 1024 * 1024);
    let mut surface = surfaces::raster_n32_premul((CARD_WIDTH, 220)).expect("画布");
    {
        let kit = CanvasKit::new(surface.canvas(), &context);
        let style = TextStyle::new(&alias, 48);
        let metrics = kit
            .measure("PLUGIN FONT 48", &style)
            .expect("可测量插件字体");
        assert!(metrics.width > 0.0, "插件字体必须给出宽度");
    }
    let scores = scores();
    let mut card_surface = surfaces::raster_n32_premul((CARD_WIDTH, CARD_HEIGHT)).expect("画布");
    {
        let kit = CanvasKit::new(card_surface.canvas(), &context);
        draw_card_with_assets(&kit, "alice", &scores, average(&scores), None, Some(&alias));
    }
    let image = card_surface.image_snapshot();
    let pixmap = image.peek_pixels().expect("像素可读");
    let pixels = pixmap.bytes().expect("像素字节");
    let bright = (40..120)
        .flat_map(|y| (48..820).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let offset = y * pixmap.row_bytes() + x * 4;
            let sum = pixels[offset] as u32 + pixels[offset + 1] as u32 + pixels[offset + 2] as u32;
            sum > 400
        })
        .count();
    assert!(
        bright > 500,
        "插件字体必须真的画出标题字形, 实际 {bright} 个亮点"
    );
}

#[test]
fn legacy_painter_matches_the_assetless_path() {
    let scores = scores();
    let average = average(&scores);
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 8 * 1024 * 1024);
    let mut legacy = surfaces::raster_n32_premul((CARD_WIDTH, CARD_HEIGHT)).expect("画布");
    let mut assetless = surfaces::raster_n32_premul((CARD_WIDTH, CARD_HEIGHT)).expect("画布");
    {
        let kit = CanvasKit::new(legacy.canvas(), &context);
        draw_card(&kit, "alice", &scores, average);
    }
    {
        let kit = CanvasKit::new(assetless.canvas(), &context);
        draw_card_with_assets(&kit, "alice", &scores, average, None, None);
    }
    let left = legacy.image_snapshot();
    let right = assetless.image_snapshot();
    let left_bytes = left
        .peek_pixels()
        .expect("像素")
        .bytes()
        .expect("字节")
        .to_vec();
    let right_bytes = right
        .peek_pixels()
        .expect("像素")
        .bytes()
        .expect("字节")
        .to_vec();
    assert_eq!(
        left_bytes, right_bytes,
        "无素材时的新路径必须与旧路径逐像素一致"
    );
}
