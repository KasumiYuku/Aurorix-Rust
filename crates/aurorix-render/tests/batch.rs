use std::sync::Arc;
use std::time::Instant;

use aurorix_render::{
    content_key, encode_rgba, CanvasKit, Color, EncodeOptions, PlacedText, RecordedLayer,
    RenderContext, RenderFormat, SkiaBackend, Sprite, TextStyle, TextureCache,
};
use bytes::Bytes;
use skia_safe::{
    canvas::SrcRectConstraint, surfaces, AlphaType, ColorType, Data, Image, ImageInfo, Rect,
};

const GUARD_MS: u128 = 30_000;

const AA_TOLERANCE: u8 = 8;

const MERGED_PATH_TOLERANCE: u8 = 40;

struct Frame {
    width: i32,
    height: i32,
    row: usize,
    pixels: Vec<u8>,
}

impl Frame {
    fn pixel(&self, x: i32, y: i32) -> (u8, u8, u8, u8) {
        assert!(
            x >= 0 && y >= 0 && x < self.width && y < self.height,
            "像素坐标越界: {x},{y}"
        );
        let offset = y as usize * self.row + x as usize * 4;
        (
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        )
    }

    fn painted(&self) -> usize {
        painted(&self.pixels)
    }
}

fn painted(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] > 0)
        .count()
}

fn context() -> Arc<RenderContext> {
    RenderContext::new(Arc::new(SkiaBackend::new()), 32, 32 * 1024 * 1024)
}

fn max_delta(alpha: &[u8], beta: &[u8]) -> u8 {
    alpha
        .iter()
        .zip(beta)
        .map(|(left, right)| left.abs_diff(*right))
        .max()
        .unwrap_or(0)
}

fn differing(alpha: &[u8], beta: &[u8]) -> usize {
    alpha
        .as_chunks::<4>()
        .0
        .iter()
        .zip(beta.as_chunks::<4>().0)
        .filter(|(left, right)| left != right)
        .count()
}

fn render_with(
    context: &Arc<RenderContext>,
    width: i32,
    height: i32,
    painter: impl FnOnce(&CanvasKit),
) -> Frame {
    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let row = info.min_row_bytes();
    let mut pixels = vec![0u8; info.compute_byte_size(row)];
    {
        let mut surface =
            surfaces::wrap_pixels(&info, &mut pixels, row, None).expect("画布应当可包装");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::TRANSPARENT);
        let kit = CanvasKit::new(canvas, context);
        painter(&kit);
    }
    Frame {
        width,
        height,
        row,
        pixels,
    }
}

fn render(width: i32, height: i32, painter: impl FnOnce(&CanvasKit)) -> Frame {
    render_with(&context(), width, height, painter)
}

fn png(seed: u8, width: i32, height: i32) -> Bytes {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            rgba.push(seed.wrapping_add(x as u8));
            rgba.push(seed.wrapping_add(y as u8));
            rgba.push(seed.wrapping_mul(3).wrapping_add(64));
            rgba.push(255);
        }
    }
    encode_rgba(
        RenderFormat::Png,
        &rgba,
        width,
        height,
        width as usize * 4,
        &EncodeOptions::new(80),
    )
    .expect("PNG 编码应当成功")
}

fn quad_image() -> Image {
    let size = 64i32;
    let half = size / 2;
    let info = ImageInfo::new((size, size), ColorType::RGBA8888, AlphaType::Premul, None);
    let row = info.min_row_bytes();
    let mut buffer = vec![0u8; info.compute_byte_size(row)];
    for y in 0..size {
        for x in 0..size {
            let offset = y as usize * row + x as usize * 4;
            let (red, green, blue) = match (x < half, y < half) {
                (true, true) => (230u8, 60u8, 60u8),
                (false, true) => (60u8, 200u8, 90u8),
                (true, false) => (70u8, 110u8, 230u8),
                (false, false) => (240u8, 210u8, 70u8),
            };
            buffer[offset] = red;
            buffer[offset + 1] = green;
            buffer[offset + 2] = blue;
            buffer[offset + 3] = 255;
        }
    }
    skia_safe::images::raster_from_data(&info, Data::new_copy(&buffer), row)
        .expect("四象限贴图应当可构造")
}

fn grid(count: usize, width: f32, height: f32, columns: usize) -> Vec<Rect> {
    (0..count)
        .map(|index| {
            let column = index % columns;
            let row = index / columns;
            Rect::from_xywh(
                column as f32 * (width + 6.0),
                row as f32 * (height + 6.0),
                width,
                height,
            )
        })
        .collect()
}

#[test]
fn repeated_bytes_decode_exactly_once() {
    let cache = TextureCache::new(8 * 1024 * 1024);
    let bytes = png(7, 64, 64);
    let mut first = None;
    for _ in 0..50 {
        let texture = cache.image_for(bytes.clone()).expect("纹理应当可获取");
        assert!(texture.image().is_some(), "命中后必须带已解码位图");
        if let Some(previous) = &first {
            assert!(
                Arc::ptr_eq(previous, &texture),
                "同一份字节必须复用同一个纹理句柄"
            );
        } else {
            first = Some(Arc::clone(&texture));
        }
    }
    assert_eq!(cache.decodes(), 1, "50 次取图只允许解码 1 次");
    let stats = cache.stats();
    assert_eq!(stats.entries, 1, "同一份字节只占一条缓存");
    assert_eq!(stats.hits, 49, "后 49 次都应当命中");
    assert_eq!(stats.misses, 1, "只有首次未命中");
    assert_eq!(stats.decodes, 1, "统计里也要体现只解码一次");
}

#[test]
fn content_key_tracks_bytes() {
    let alpha = content_key(b"aurorix-alpha");
    let beta = content_key(b"aurorix-beta");
    assert_eq!(
        alpha,
        content_key(b"aurorix-alpha"),
        "同一份字节的键必须稳定"
    );
    assert_ne!(alpha, beta, "不同字节的键必须不同");
    assert!(alpha.starts_with("blob-"), "内容键需带可识别前缀");
}

#[test]
fn cache_evicts_oldest_when_capacity_is_exceeded() {
    let unit = png(11, 32, 32);
    let cache = TextureCache::new(unit.len() as u64 * 2 + 8);
    cache.image_for(unit.clone()).expect("第一张应当写入");
    cache.image_for(png(12, 32, 32)).expect("第二张应当写入");
    let newest = cache.image_for(png(13, 32, 32)).expect("第三张应当写入");
    assert!(newest.image().is_some(), "新写入的纹理必须可绘制");
    let stats = cache.stats();
    assert!(stats.entries <= 2, "条目数不能超过容量允许的数量");
    assert!(stats.evicted >= 1, "超出容量后必须淘汰最旧纹理");
}

#[test]
fn drop_decoded_keeps_encoded_bytes() {
    let cache = TextureCache::new(8 * 1024 * 1024);
    let bytes = png(31, 48, 48);
    cache.image_for(bytes.clone()).expect("纹理应当可获取");
    assert_eq!(cache.drop_decoded(), 1, "应丢掉一份已解码位图");
    let stats = cache.stats();
    assert_eq!(stats.entries, 1, "淘汰位图不能丢掉编码字节");
    assert_eq!(stats.bytes, bytes.len() as u64, "编码字节数必须保持不变");
    let again = cache.image_for(bytes.clone()).expect("再次取图应当成功");
    assert!(again.image().is_some(), "重新取图应当带回位图");
    assert_eq!(cache.decodes(), 2, "丢掉位图后允许再解码一次");
}

#[test]
fn rect_batch_matches_individual_fills() {
    let rects = grid(50, 14.0, 10.0, 10);
    let color = Color::rgb(40, 120, 200);
    let batched = render(200, 80, |kit| {
        kit.fill_rects_batch(&rects, color);
    });
    let individual = render(200, 80, |kit| {
        for rect in &rects {
            kit.fill_rect(*rect, color);
        }
    });
    assert!(batched.painted() > 0, "批量矩形必须真的画上颜色");
    assert_eq!(
        batched.pixels, individual.pixels,
        "批量矩形的像素必须与逐个填充完全一致"
    );
}

#[test]
fn round_rect_batch_matches_individual_fills() {
    let items: Vec<(Rect, f32)> = grid(50, 16.0, 12.0, 10)
        .into_iter()
        .map(|rect| (rect, 5.0))
        .collect();
    let color = Color::rgb(230, 90, 60);
    let batched = render(220, 90, |kit| {
        kit.fill_round_rects_batch(&items, color);
    });
    let individual = render(220, 90, |kit| {
        for (rect, radius) in &items {
            kit.rrect_fill(*rect, *radius, color);
        }
    });
    assert!(batched.painted() > 0, "批量圆角矩形必须真的画上颜色");
    let delta = max_delta(&batched.pixels, &individual.pixels);
    let diff = differing(&batched.pixels, &individual.pixels);
    assert!(
        delta <= MERGED_PATH_TOLERANCE && diff * 4 <= batched.painted(),
        "批量圆角矩形与逐个绘制的差异过大: 最大通道差 {delta}, 差异像素 {diff}"
    );
}

#[test]
fn sprite_batch_matches_individual_draws() {
    let image = quad_image();
    let sprites: Vec<Sprite> = [
        Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
        Rect::from_xywh(32.0, 0.0, 32.0, 32.0),
        Rect::from_xywh(0.0, 32.0, 32.0, 32.0),
        Rect::from_xywh(32.0, 32.0, 32.0, 32.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, source)| Sprite::from_size(source, 8.0 + index as f32 * 48.0, 8.0, 40.0, 40.0))
    .collect();
    let batched = render(220, 60, |kit| {
        kit.draw_sprites_batch(&image, &sprites);
    });
    let individual = render(220, 60, |kit| {
        let mut paint = skia_safe::Paint::default();
        paint.set_anti_alias(true);
        paint.set_dither(true);
        for sprite in &sprites {
            kit.canvas().draw_image_rect_with_sampling_options(
                &image,
                Some((&sprite.src, SrcRectConstraint::Fast)),
                sprite.dest,
                kit.sampling(),
                &paint,
            );
        }
    });
    assert!(batched.painted() > 0, "批量精灵必须真的画上颜色");
    let delta = max_delta(&batched.pixels, &individual.pixels);
    let diff = differing(&batched.pixels, &individual.pixels);
    assert!(
        delta <= AA_TOLERANCE && diff * 20 <= batched.painted(),
        "批量精灵与逐个绘制的差异过大: 最大通道差 {delta}, 差异像素 {diff}"
    );
}

#[test]
fn text_batch_reuses_cached_blobs() {
    let context = context();
    let style = TextStyle::new("sans-serif", 18);
    let items: Vec<PlacedText> = (0..6)
        .map(|index| PlacedText::new(format!("格{index} 12.34"), 8.0, 20.0 + index as f32 * 20.0))
        .collect();
    for _ in 0..2 {
        let frame = render_with(&context, 200, 140, |kit| {
            let drawn = kit
                .draw_texts_batch(&style, &items)
                .expect("批量文字应当绘制成功");
            assert_eq!(drawn, items.len(), "六个标签都应当提交绘制");
        });
        assert!(frame.painted() > 0, "批量文字必须真的画上颜色");
    }
    let mut shaped = 0;
    let mut hits = 0;
    let probe = render_with(&context, 200, 140, |kit| {
        kit.draw_texts_batch(&style, &items)
            .expect("批量文字应当绘制成功");
        let stats = kit.text_blob_cache().stats();
        shaped = stats.shaped;
        hits = stats.hits;
    });
    assert!(probe.painted() > 0, "批量文字必须真的画上颜色");
    assert_eq!(shaped, 6, "六个不同标签只该排版六次");
    assert_eq!(hits, 12, "后两轮共 12 次全部命中排版缓存");
}

#[test]
fn recorded_layer_replays_identically() {
    let bounds = Rect::from_xywh(0.0, 0.0, 200.0, 100.0);
    let layer = RecordedLayer::record(bounds, |canvas| {
        let mut paint = skia_safe::Paint::default();
        paint.set_color(skia_safe::Color::from_argb(255, 30, 90, 160));
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 200.0, 40.0), &paint);
        paint.set_color(skia_safe::Color::from_argb(255, 220, 200, 40));
        canvas.draw_rect(Rect::from_xywh(0.0, 40.0, 200.0, 60.0), &paint);
    })
    .expect("静态图层应当录制成功");
    assert_eq!(layer.bounds(), bounds, "录制后边界必须保持一致");
    let replayed = render(200, 100, |kit| {
        layer.draw(kit.canvas());
    });
    let blank = render(200, 100, |_kit| {});
    assert_eq!(blank.painted(), 0, "空画布不该有内容, 说明对比基准干净");
    assert_eq!(
        replayed.painted(),
        (200 * 100) as usize,
        "重放静态图层必须铺满整幅画布"
    );
    let top = replayed.pixel(100, 20);
    let bottom = replayed.pixel(100, 70);
    assert_eq!(top.0, 30, "上半区颜色必须来自录制内容");
    assert_eq!(bottom.0, 220, "下半区颜色必须来自录制内容");
    let shifted = render(200, 100, |kit| {
        layer.draw_at(kit.canvas(), skia_safe::Point::new(0.0, 0.0));
    });
    assert_eq!(
        replayed.pixels, shifted.pixels,
        "零偏移重放必须与直接重放一致"
    );
}

#[test]
fn b50_card_prefers_cached_batch() {
    let started = Instant::now();
    let context = context();
    let cell_png = png(21, 96, 96);
    let cells = grid(50, 34.0, 26.0, 10);
    let info = ImageInfo::new((420, 180), ColorType::RGBA8888, AlphaType::Premul, None);
    let row = info.min_row_bytes();
    let mut sequential_pixels = vec![0u8; info.compute_byte_size(row)];
    let sequential_started = Instant::now();
    {
        let mut surface =
            surfaces::wrap_pixels(&info, &mut sequential_pixels, row, None).expect("逐格画布");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::TRANSPARENT);
        let kit = CanvasKit::new(canvas, &context);
        for cell in &cells {
            let image = Image::from_encoded(Data::new_copy(&cell_png)).expect("逐格解码");
            kit.image_rect(&image, *cell);
        }
    }
    let sequential = sequential_started.elapsed();
    let mut batched_pixels = vec![0u8; info.compute_byte_size(row)];
    let batched_started = Instant::now();
    let decodes = {
        let mut surface =
            surfaces::wrap_pixels(&info, &mut batched_pixels, row, None).expect("批量画布");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::TRANSPARENT);
        let kit = CanvasKit::new(canvas, &context);
        let cache = context.textures();
        for _ in &cells {
            cache.image_for(cell_png.clone()).expect("缓存取图");
        }
        let first = cache.image_for(cell_png.clone()).expect("首张纹理");
        let image = first.image().expect("首张纹理应当已解码");
        let (source_width, source_height) = (image.width() as f32, image.height() as f32);
        let sprites: Vec<Sprite> = cells
            .iter()
            .map(|cell| {
                Sprite::new(
                    Rect::from_xywh(0.0, 0.0, source_width, source_height),
                    *cell,
                )
            })
            .collect();
        kit.draw_sprites_batch(image, &sprites);
        cache.decodes()
    };
    let batched = batched_started.elapsed();
    assert_eq!(decodes, 1, "50 格同图只允许解码 1 次");
    let painted_sequential = painted(&sequential_pixels);
    let painted_batched = painted(&batched_pixels);
    assert!(painted_sequential > 0, "逐格绘制必须有内容");
    assert_eq!(
        painted_batched, painted_sequential,
        "批量绘制的覆盖面积必须与逐格绘制一致"
    );
    assert!(
        batched <= sequential,
        "批量加缓存耗时 {:?} 不应慢于逐格解码 {:?}",
        batched,
        sequential
    );
    assert!(
        started.elapsed().as_millis() < GUARD_MS,
        "整段基准必须快速跑完"
    );
}

#[test]
fn regular_draw_image_decodes_once_for_repeated_bytes() {
    let context = context();
    let icon = png(7, 48, 48);
    let bytes: &[u8] = icon.as_ref();
    let frame = render_with(&context, 200, 60, |kit| {
        for index in 0..20 {
            let left = 6.0 + index as f32 * 9.0;
            kit.draw_image(bytes, Rect::from_xywh(left, 6.0, 8.0, 8.0))
                .expect("常规绘制应当复用纹理缓存");
        }
    });
    let stats = context.textures().stats();
    assert_eq!(stats.decodes, 1, "同一张图绘制 20 次只允许解码 1 次");
    assert_eq!(context.textures().decodes(), 1, "解码计数必须保持一致");
    assert!(stats.hits >= 19, "后 19 次绘制必须命中缓存");
    assert_eq!(stats.entries, 1, "缓存里只应留下一份纹理");
    assert!(frame.painted() > 0, "画面必须有内容");
}

#[test]
fn regular_text_draw_reuses_text_blob_cache() {
    let context = context();
    let style = TextStyle::new("Noto Sans CJK JP", 18);
    let frame = render_with(&context, 240, 80, |kit| {
        for index in 0..10 {
            kit.draw_text("正在查询", 8.0, 20.0 + index as f32 * 6.0, &style)
                .expect("文本绘制应当成功");
        }
    });
    let stats = context.text_blobs().stats();
    assert_eq!(stats.shaped, 1, "同一段短文本只允许排版一次");
    assert_eq!(stats.hits, 9, "后续 9 次绘制必须复用排版结果");
    assert!(frame.painted() > 0, "文本必须有可见像素");
}
