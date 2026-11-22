use std::sync::Arc;
use std::time::Instant;

use aurorix_render::{
    CanvasKit, Color, EncodeOptions, RenderContext, RenderQuality, RenderRequest, SkiaBackend,
};
use example_rating::metrics::{analyze, RgbaImage};
use example_rating::{
    average_rating, card_font_chain, demo_scores, draw_card, emit_sample_outputs,
    sample_output_dir, Score, CARD_HEIGHT, CARD_SCALE, CARD_WIDTH,
};

fn render(
    context: &RenderContext,
    request: RenderRequest,
    quality: RenderQuality,
    user: &str,
    scores: &[Score],
    average: f64,
) -> (Vec<u8>, f64) {
    let options: EncodeOptions = quality.encode_options();
    let started = Instant::now();
    let bytes = context
        .draw_with_options(quality.apply(request), options, |canvas, context| {
            let kit = CanvasKit::with_chain(canvas, context, card_font_chain());
            draw_card(&kit, user, scores, average);
        })
        .expect("渲染失败");
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    (bytes.to_vec(), elapsed)
}

fn kib(bytes: usize) -> f64 {
    bytes as f64 / 1024.0
}

fn main() {
    let scores = demo_scores();
    let average = average_rating(&scores, scores.len());
    let request = RenderRequest::new(CARD_WIDTH, CARD_HEIGHT)
        .with_scale(CARD_SCALE)
        .with_background(Color::rgb(10, 12, 22));
    let (physical_width, physical_height) = request.physical_size();
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 64, 96 * 1024 * 1024);

    let (png, png_ms) = render(
        &context,
        request,
        RenderQuality::CrispPng,
        "alice",
        &scores,
        average,
    );
    let (webp, webp_ms) = render(
        &context,
        request,
        RenderQuality::HighWebp,
        "alice",
        &scores,
        average,
    );

    println!(
        "logical {}x{} @ {:.1}x -> physical {}x{}",
        CARD_WIDTH, CARD_HEIGHT, CARD_SCALE, physical_width, physical_height
    );
    println!(
        "PNG  {} bytes ({:.1} KiB) in {:.1} ms",
        png.len(),
        kib(png.len()),
        png_ms
    );
    println!(
        "WebP {} bytes ({:.1} KiB) in {:.1} ms",
        webp.len(),
        kib(webp.len()),
        webp_ms
    );

    let decoded = RgbaImage::decode(&png).expect("PNG 解码失败");
    let analysis = analyze(&decoded, CARD_SCALE);
    println!(
        "解码尺寸 {}x{} · 独立颜色 {}",
        analysis.width, analysis.height, analysis.distinct_colors
    );
    println!(
        "文字带 方差 {:.1} · 水平边缘 delta 均值 {:.2} / p95 {:.1} · 亮度 {:.1}..{:.1}",
        analysis.text_band.variance,
        analysis.text_band.edge_delta,
        analysis.text_band.edge_peak,
        analysis.text_band.min,
        analysis.text_band.max
    );
    println!(
        "纯背景带 方差 {:.1} · 水平边缘 delta 均值 {:.2} / p95 {:.1} · 亮度 {:.1}..{:.1}",
        analysis.background_band.variance,
        analysis.background_band.edge_delta,
        analysis.background_band.edge_peak,
        analysis.background_band.min,
        analysis.background_band.max
    );
    println!(
        "难度色块区域 {:?} · 最大色度 {} · 高饱和像素 {} (超过阈值 60 的通道差)",
        analysis.pill_band, analysis.pill_max_chroma, analysis.pill_saturated_pixels
    );
    println!("出现最多的 5 种颜色:");
    for (color, count) in &analysis.top_colors {
        println!(
            "  #{:02X}{:02X}{:02X}  {} 次 ({:.2}%)",
            color[0],
            color[1],
            color[2],
            count,
            (*count as f64) * 100.0 / (analysis.width * analysis.height) as f64
        );
    }

    match sample_output_dir() {
        Some(dir) => {
            let written = emit_sample_outputs(Some(&dir), &png, &webp).expect("写出示例图失败");
            if let Some(written) = written {
                println!(
                    "已写出 {} 与 {}",
                    written.png.display(),
                    written.webp.display()
                );
            }
        }
        None => println!("未设置 AURORIX_SAMPLE_OUT, 不写出示例文件 (CI 不会改动 docs/)"),
    }
}
