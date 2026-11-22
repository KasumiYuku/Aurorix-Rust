use std::sync::Arc;
use std::time::{Duration, Instant};

use aurorix_render::{
    CanvasKit, Color, EncodeOptions, FontChain, FontWeight, GradientStop, LinearGradient, Padding,
    PoolStats, RadialGradient, RenderContext, RenderError, RenderFormat, RenderPool,
    RenderPoolConfig, RenderPriority, RenderRequest, SharedImage, SkiaBackend, SurfaceReuseStats,
    TextAlign, TextStyle,
};
use bytes::Bytes;
use skia_safe::{surfaces, AlphaType, BlendMode, ColorType, Data, Image, ImageInfo, Rect};

const HI_WIDTH: i32 = 2000;
const HI_HEIGHT: i32 = 3000;
const SHEET_WIDTH: i32 = 1600;
const SHEET_HEIGHT: i32 = 2400;
const CARDS: usize = 50;
const COLUMNS: usize = 5;
const CONCURRENCY: usize = 20;
const PNG_BUDGET: Duration = Duration::from_secs(120);

const KANA_LINES: [&str; 5] = [
    "アイウエオ ★♪",
    "カキクケコ ☆∞",
    "サシスセソ ◆◇",
    "タチツテト ♪★",
    "ナニヌネノ ※→",
];

const TITLE_LINES: [&str; 5] = [
    "Testify",
    "Aurorix",
    "Sound Chimera",
    "PUPA",
    "Grievous Lady",
];

fn scene_style() -> Vec<TextStyle> {
    vec![
        TextStyle::new("DejaVu Sans", 26)
            .with_weight(FontWeight::Bold)
            .with_color(Color::WHITE),
        TextStyle::new("DejaVu Sans", 18)
            .with_weight(FontWeight::Normal)
            .with_color(Color::rgb(232, 234, 246)),
        TextStyle::new("DejaVu Sans", 22)
            .with_weight(FontWeight::SemiBold)
            .with_color(Color::rgb(255, 214, 102))
            .with_letter_spacing(0.4),
        TextStyle::new("DejaVu Sans", 16)
            .with_weight(FontWeight::Light)
            .with_color(Color::rgb(200, 205, 220))
            .with_align(TextAlign::Right),
    ]
}

struct Scene {
    covers: Vec<Arc<SharedImage>>,
    titles: Vec<TextStyle>,
    chain: FontChain,
}

impl Scene {
    fn build() -> Self {
        let mut covers = Vec::new();
        for index in 0..COLUMNS {
            covers.push(Arc::new(SharedImage::from_image(cover(index))));
        }
        Self {
            covers,
            titles: scene_style(),
            chain: FontChain::with_default_fallbacks("DejaVu Sans"),
        }
    }

    fn cover(&self, column: usize) -> Image {
        self.covers[column % self.covers.len()].duplicate()
    }
}

fn cover(seed: usize) -> Image {
    let side = 320;
    let info = ImageInfo::new((side, side), ColorType::RGBA8888, AlphaType::Premul, None);
    let row = info.min_row_bytes();
    let mut buffer = vec![0u8; info.compute_byte_size(row)];
    {
        let mut surface = surfaces::wrap_pixels(&info, &mut buffer, row, None).expect("贴图画布");
        let canvas = surface.canvas();
        let gradient = LinearGradient::new([
            GradientStop::new(0.0, Color::rgb((40 + seed * 30) as u8, 60, 200)),
            GradientStop::new(1.0, Color::rgb(220, (90 + seed * 20) as u8, 140)),
        ])
        .with_angle(60.0);
        let mut paint = skia_safe::Paint::default();
        paint.set_anti_alias(true);
        paint.set_shader(
            gradient
                .shader(Rect::from_xywh(0.0, 0.0, side as f32, side as f32))
                .expect("贴图渐变"),
        );
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, side as f32, side as f32), &paint);
        let mut accent = skia_safe::Paint::default();
        accent.set_anti_alias(true);
        accent.set_color(skia_safe::Color::from_argb(180, 255, 255, 255));
        for step in 0..6 {
            let offset = (step * 46) as f32;
            canvas.draw_circle((offset + 40.0, 90.0 + offset), 26.0, &accent);
        }
    }
    skia_safe::images::raster_from_data(&info, Data::new_copy(&buffer), row)
        .expect("贴图应当可构造")
}

fn card_geometry(index: usize) -> Rect {
    let margin = 32.0f32;
    let gap = 16.0f32;
    let top = 148.0f32;
    let column = index % COLUMNS;
    let row = index / COLUMNS;
    let width = (SHEET_WIDTH as f32 - margin * 2.0 - gap * (COLUMNS as f32 - 1.0)) / COLUMNS as f32;
    let rows = (CARDS / COLUMNS) as f32;
    let height = (SHEET_HEIGHT as f32 - top - margin - gap * (rows - 1.0)) / rows;
    Rect::from_xywh(
        margin + column as f32 * (width + gap),
        top + row as f32 * (height + gap),
        width,
        height,
    )
}

fn paint_sheet(context: &RenderContext, scene: &Scene, canvas: &skia_safe::Canvas) {
    let kit = CanvasKit::with_chain(canvas, context, scene.chain.clone());
    kit.fill_rect(
        Rect::from_xywh(0.0, 0.0, SHEET_WIDTH as f32, SHEET_HEIGHT as f32),
        Color::rgb(14, 16, 28),
    );
    let header = LinearGradient::new([
        GradientStop::new(0.0, Color::rgb(26, 30, 54)),
        GradientStop::new(1.0, Color::rgb(52, 20, 60)),
    ])
    .with_angle(20.0);
    kit.fill_gradient(
        Rect::from_xywh(24.0, 24.0, SHEET_WIDTH as f32 - 48.0, 96.0),
        18.0,
        &header.into(),
    )
    .expect("表头渐变应当可绘制");
    kit.draw_text(
        "Aurorix B50 成绩总览 ★♪",
        48.0,
        88.0,
        &scene.titles[0].clone().with_size(34),
    )
    .expect("表头文本应当可绘制");
    let badge = kit
        .badge_at(
            skia_safe::Point::new(SHEET_WIDTH as f32 - 300.0, 56.0),
            "Rating 13,842",
            &scene.titles[2],
            Color::rgb(255, 200, 60),
            Color::rgb(20, 20, 20),
            Padding::new(18.0, 10.0),
        )
        .expect("徽章应当可绘制");
    assert!(badge.width() > 0.0, "徽章宽度应当为正");

    kit.layer(
        Rect::from_xywh(0.0, 0.0, SHEET_WIDTH as f32, SHEET_HEIGHT as f32),
        0.96,
        BlendMode::Multiply,
        |kit| {
            for index in 0..CARDS {
                let card = card_geometry(index);
                let column = index % COLUMNS;
                let seed = index % 5;
                let fill = LinearGradient::new([
                    GradientStop::new(0.0, Color::rgb(38, 42, 66)),
                    GradientStop::new(1.0, Color::rgb(22 + seed as u8 * 4, 26, 46)),
                ])
                .with_angle(110.0);
                kit.drop_shadow_in(card, Color::new(0, 0, 0, 170), 0.0, 8.0, 10.0, |kit| {
                    kit.fill_gradient(card, 18.0, &fill.into())
                        .expect("卡片渐变应当可绘制");
                    let cover_rect = Rect::from_xywh(
                        card.left() + 14.0,
                        card.top() + 14.0,
                        card.height() - 28.0,
                        card.height() - 28.0,
                    );
                    kit.rounded_image(&scene.cover(column), cover_rect, 14.0);
                });
                let text_x = card.left() + card.height() + 4.0;
                let max_width = card.right() - text_x - 12.0;
                let (fitted, style) =
                    kit.fit_to_width(TITLE_LINES[seed], &scene.titles[0], max_width);
                kit.draw_text(&fitted, text_x, card.top() + 42.0, &style)
                    .expect("标题应当可绘制");
                let (kana, kana_style) =
                    kit.fit_to_width(KANA_LINES[seed], &scene.titles[1], max_width);
                kit.draw_text(&kana, text_x, card.top() + 78.0, &kana_style)
                    .expect("假名文本应当可绘制");
                let (score, score_style) =
                    kit.fit_to_width("12,876 AAA ★", &scene.titles[2], max_width);
                kit.draw_text(&score, text_x, card.top() + 118.0, &score_style)
                    .expect("分数文本应当可绘制");
                kit.draw_text(
                    "♪ PURE MEMORY",
                    text_x,
                    card.top() + 156.0,
                    &scene.titles[3].clone().with_align(TextAlign::Left),
                )
                .expect("评级文本应当可绘制");
                let pip = RadialGradient::new([
                    GradientStop::new(0.0, Color::rgb(255, 236, 160)),
                    GradientStop::new(1.0, Color::rgb(255, 160, 40)),
                ]);
                kit.fill_gradient(
                    Rect::from_xywh(card.right() - 42.0, card.bottom() - 42.0, 28.0, 28.0),
                    14.0,
                    &pip.into(),
                )
                .expect("角标应当可绘制");
            }
        },
    );
}

fn render_sheet(
    context: &RenderContext,
    scene: &Scene,
    format: RenderFormat,
    quality: u8,
) -> Result<Bytes, RenderError> {
    let request = RenderRequest::new(SHEET_WIDTH, SHEET_HEIGHT)
        .with_format(format)
        .with_background(Color::rgb(10, 12, 20));
    context.draw_with_options(request, EncodeOptions::new(quality), |canvas, context| {
        paint_sheet(context, scene, canvas);
    })
}

fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() as f64 * fraction).ceil() as usize).clamp(1, sorted.len()) - 1;
    sorted[index]
}

fn memory_megabytes() -> (f64, f64) {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return (0.0, 0.0);
    };
    let mut peak = 0.0;
    let mut current = 0.0;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            peak = parse_kilobytes(rest);
        } else if let Some(rest) = line.strip_prefix("VmRSS:") {
            current = parse_kilobytes(rest);
        }
    }
    (peak / 1024.0, current / 1024.0)
}

fn parse_kilobytes(text: &str) -> f64 {
    text.split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn thread_report() -> String {
    match std::thread::available_parallelism() {
        Ok(value) => value.get().to_string(),
        Err(_) => String::from("unknown"),
    }
}

#[test]
#[ignore = "压测基准: cargo test -p aurorix-render --test bench_b50 -- --ignored --nocapture"]
fn bench_b50_sheet() {
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 64, 64 * 1024 * 1024);
    let scene = Scene::build();
    let (peak_before, rss_before) = memory_megabytes();
    println!("==== Aurorix B50 压测 ====");
    println!(
        "机器: 可用并行度 {} | 画布 {}x{} | 卡片 {} 张 | 文本约 {} 行",
        thread_report(),
        SHEET_WIDTH,
        SHEET_HEIGHT,
        CARDS,
        CARDS * 4
    );
    println!("起始内存: VmHWM {peak_before:.1} MB / RSS {rss_before:.1} MB\n");

    let started = Instant::now();
    let warmup = render_sheet(&context, &scene, RenderFormat::Png, 90).expect("预热渲染应当成功");
    println!(
        "预热: 单张 PNG 渲染+编码 {:.1} ms ({} 字节)\n",
        ms(started),
        warmup.len()
    );

    for (format, quality) in [
        (RenderFormat::Png, 90u8),
        (RenderFormat::Jpeg, 85),
        (RenderFormat::WebP, 80),
    ] {
        let started = Instant::now();
        let bytes = render_sheet(&context, &scene, format, quality).expect("单张渲染应当成功");
        println!(
            "单张 {} q{}: {:.1} ms | {} 字节 ({:.1} KiB)",
            format.name(),
            quality,
            ms(started),
            bytes.len(),
            bytes.len() as f64 / 1024.0
        );
    }

    let row_bytes = SHEET_WIDTH as usize * 4;
    let mut rgba = vec![0u8; row_bytes * SHEET_HEIGHT as usize];
    for (index, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = (index % row_bytes / 4) as u32;
        let y = (index / row_bytes) as u32;
        pixel[0] = ((x * 3) % 256) as u8;
        pixel[1] = ((y * 5) % 256) as u8;
        pixel[2] = ((x + y) % 256) as u8;
        pixel[3] = 255;
    }
    println!("\n纯编码耗时 (1600x2400 RGBA 输入, 渲染成本不计入):");
    for (format, quality, lossless, method) in [
        (RenderFormat::Png, 90u8, false, 0u8),
        (RenderFormat::Jpeg, 85, false, 0),
        (RenderFormat::WebP, 80, false, 2),
        (RenderFormat::WebP, 80, false, 4),
        (RenderFormat::WebP, 80, false, 6),
        (RenderFormat::WebP, 80, true, 2),
    ] {
        let mut options = EncodeOptions::new(quality);
        if format == RenderFormat::WebP {
            options = options.with_webp_method(method).with_lossless(lossless);
        }
        let started = Instant::now();
        let bytes = aurorix_render::encode_rgba(
            format,
            &rgba,
            SHEET_WIDTH,
            SHEET_HEIGHT,
            row_bytes,
            &options,
        )
        .expect("纯编码应当成功");
        let label = if format == RenderFormat::WebP {
            format!(
                "{} q{}{} m{}",
                format.name(),
                quality,
                if lossless { " 无损" } else { "" },
                method
            )
        } else {
            format!("{} q{}", format.name(), quality)
        };
        println!("  {label:<16} {:.1} ms | {} 字节", ms(started), bytes.len());
    }
    drop(rgba);

    let reuse_before = aurorix_render::surface_reuse_stats();
    let pool =
        RenderPool::with_config(RenderPoolConfig::default().with_queue_capacity(CONCURRENCY * 2))
            .expect("线程池应当创建成功");
    println!(
        "\n并发 {} 张 (线程池 {} 线程, {} 并发):",
        CONCURRENCY,
        pool.thread_count(),
        CONCURRENCY
    );
    let started = Instant::now();
    let mut tickets = Vec::with_capacity(CONCURRENCY);
    for _ in 0..CONCURRENCY {
        let context = Arc::clone(&context);
        let scene_covers = scene.covers.clone();
        let titles = scene.titles.clone();
        let chain = scene.chain.clone();
        tickets.push(
            pool.submit(
                RenderPriority::High,
                Box::new(move || {
                    let local = Scene {
                        covers: scene_covers,
                        titles,
                        chain,
                    };
                    let task_started = Instant::now();
                    let bytes = render_sheet(&context, &local, RenderFormat::Png, 90)?;
                    Ok(Bytes::from(format!(
                        "{}|{:.3}",
                        bytes.len(),
                        task_started.elapsed().as_secs_f64() * 1000.0
                    )))
                }),
            )
            .expect("并发任务应当提交成功"),
        );
    }
    let mut latencies = Vec::with_capacity(CONCURRENCY);
    let mut total_bytes = 0usize;
    for ticket in tickets {
        let payload = ticket.wait_blocking().expect("并发渲染应当成功");
        let text = String::from_utf8(payload.to_vec()).expect("结果应当是合法 UTF-8");
        let (size, elapsed) = text.split_once('|').expect("结果格式应当正确");
        total_bytes += size.parse::<usize>().unwrap_or(0);
        latencies.push(elapsed.parse::<f64>().unwrap_or(0.0));
    }
    let wall = ms(started);
    assert!(
        started.elapsed() <= PNG_BUDGET,
        "压测耗时超出预算: {wall:.1} ms"
    );
    latencies.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let throughput = CONCURRENCY as f64 / (wall / 1000.0);
    println!(
        "并发总耗时 {:.1} ms | 吞吐 {:.2} 张/秒 | 平均 {:.1} ms | P50 {:.1} ms | P99 {:.1} ms",
        wall,
        throughput,
        latencies.iter().sum::<f64>() / latencies.len() as f64,
        percentile(&latencies, 0.50),
        percentile(&latencies, 0.99)
    );
    println!(
        "并发输出合计 {:.2} MiB | 平均单张 {:.2} MiB",
        total_bytes as f64 / (1024.0 * 1024.0),
        total_bytes as f64 / (1024.0 * 1024.0) / CONCURRENCY as f64
    );
    let reuse_after = aurorix_render::surface_reuse_stats();
    println!(
        "像素缓冲复用: 命中 {} | 未命中 {} | 分配 {} | 本次命中率 {:.1}%",
        reuse_after.hits - reuse_before.hits,
        reuse_after.misses - reuse_before.misses,
        reuse_after.allocations - reuse_before.allocations,
        reuse_rate(reuse_before, reuse_after)
    );
    let stats: PoolStats = pool.stats();
    println!(
        "线程池统计: 完成 {} | 失败 {} | 拒绝 {} | 在队 {}",
        stats.completed, stats.failed, stats.rejected, stats.queued
    );
    pool.shutdown();
    println!("关闭后存活线程 {}\n", pool.live_threads());

    let (peak, rss) = memory_megabytes();
    println!(
        "峰值内存: VmHWM {peak:.1} MB | 结束 RSS {rss:.1} MB | 增量 {:.1} MB",
        peak - peak_before
    );
    let reuse: SurfaceReuseStats = aurorix_render::surface_reuse_stats();
    println!(
        "进程内累计复用: 命中 {} / 未命中 {}",
        reuse.hits, reuse.misses
    );
}

fn reuse_rate(before: SurfaceReuseStats, after: SurfaceReuseStats) -> f64 {
    let hits = after.hits.saturating_sub(before.hits);
    let misses = after.misses.saturating_sub(before.misses);
    let total = hits + misses;
    if total == 0 {
        0.0
    } else {
        hits as f64 / total as f64 * 100.0
    }
}

#[test]
#[ignore = "压测基准: cargo test -p aurorix-render --test bench_b50 -- --ignored --nocapture"]
fn bench_b50_sheet_at_2000x3000() {
    let scale = (
        (HI_WIDTH as f32) / (SHEET_WIDTH as f32),
        (HI_HEIGHT as f32) / (SHEET_HEIGHT as f32),
    );
    let scene = Scene::build();
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 64 * 1024 * 1024);
    println!("\n音游大图 {HI_WIDTH}x{HI_HEIGHT} 单张 (渲染+编码, 逻辑画布 {SHEET_WIDTH}x{SHEET_HEIGHT} 放大 {:.2}x)", scale.0);
    for (format, quality) in [
        (RenderFormat::Png, 90u8),
        (RenderFormat::Jpeg, 85),
        (RenderFormat::WebP, 80),
    ] {
        let request = RenderRequest::new(HI_WIDTH, HI_HEIGHT)
            .with_format(format)
            .with_background(Color::rgb(10, 12, 20));
        let started = Instant::now();
        let bytes = context
            .draw_with_options(request, EncodeOptions::new(quality), |canvas, context| {
                canvas.save();
                canvas.scale(scale);
                paint_sheet(context, &scene, canvas);
                canvas.restore();
            })
            .expect("大图渲染应当成功");
        println!(
            "  {:?} q{quality}: {:.0} ms / {} KiB",
            format,
            ms(started),
            bytes.len() / 1024
        );
    }
    println!("线程池: {}", thread_report());
}
