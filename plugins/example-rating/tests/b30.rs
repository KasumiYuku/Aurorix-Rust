use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aurorix_core::{Scene, Target};
use aurorix_plugin::{CommandRegistry, Incoming, Outgoing, Part, RenderCard, ReplyKind, Router};
use aurorix_render::{
    CanvasKit, Color, RenderContext, RenderRequest, RenderScheduler, RenderTask, SchedulerConfig,
    SkiaBackend, TextStyle,
};
use example_rating::metrics::{analyze, RgbaImage};
use example_rating::{
    average_rating, card_font_chain, command_spec, demo_scores, emit_sample_outputs,
    sample_output_dir_from, sample_output_paths, MemoryStore, Score, CARD_HEIGHT, CARD_SCALE,
    CARD_WIDTH, COMMAND, FONT_SANS, SAMPLE_PNG_NAME, SAMPLE_WEBP_NAME,
};

fn store() -> Arc<MemoryStore> {
    let store = Arc::new(MemoryStore::new());
    store.seed("alice", demo_scores());
    store
}

fn incoming(content: &str, user: &str) -> Incoming {
    Incoming::new(Scene::Group, Target::new(Scene::Group, "g1"), user, content)
        .with_username("测试用户")
}

fn router(store: Arc<MemoryStore>) -> Router {
    let registry = CommandRegistry::new();
    registry.register_plugin(example_rating::plugin(store));
    Router::new(Arc::new(registry))
}

fn card_of(reply: &Outgoing) -> &RenderCard {
    reply
        .parts
        .iter()
        .find_map(|part| match part {
            Part::Card { card, .. } => Some(card),
            _ => None,
        })
        .expect("卡片")
}

fn physical_size() -> (usize, usize) {
    (
        (CARD_WIDTH as f32 * CARD_SCALE) as usize,
        (CARD_HEIGHT as f32 * CARD_SCALE) as usize,
    )
}

async fn render_card_png() -> Vec<u8> {
    let outcome = router(store())
        .dispatch_detailed(incoming("/b30 alice", "u1"))
        .await;
    let reply = outcome.reply.expect("回复");
    let card = card_of(&reply).clone();
    let scheduler = RenderScheduler::new(
        SchedulerConfig {
            workers: 2,
            queue_capacity: 8,
            max_in_flight_bytes: 128 * 1024 * 1024,
            task_timeout: Duration::from_secs(60),
        },
        Arc::new(SkiaBackend::new()),
    );
    let painter = Arc::clone(&card.painter);
    let request = card.request;
    let handle = scheduler
        .submit_render(
            RenderTask::new("card-check").with_estimated_bytes(request.estimated_bytes()),
            request,
            move |canvas, context| {
                let kit = CanvasKit::new(canvas, context);
                painter(&kit);
            },
        )
        .await
        .expect("提交渲染");
    let output = handle.wait().await.expect("渲染成功");
    scheduler.shutdown().await;
    output.bytes.to_vec()
}

fn docs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

fn snapshot(dir: &Path) -> Vec<(String, u64)> {
    let mut entries: Vec<(String, u64)> = std::fs::read_dir(dir)
        .map(|read| {
            read.flatten()
                .filter(|entry| entry.path().is_file())
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let size = entry.metadata().ok()?.len();
                    Some((name, size))
                })
                .collect()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

#[test]
fn plugin_registers_b30_with_alias_and_cooldown() {
    let command = command_spec(store());
    assert_eq!(command.name, COMMAND);
    assert_eq!(command.aliases, vec!["best30".to_string()]);
    assert_eq!(command.role, Some(aurorix_core::RoleRequired::Member));
    assert!(command.cooldown.enabled(), "冷却必须开启");
    assert_eq!(command.cooldown.limit, 2);
    assert_eq!(command.cooldown.window, Duration::from_secs(10));
    let args = command.args.expect("参数 spec 要声明");
    assert_eq!(args.positionals.len(), 1);
    assert!(!args.positionals[0].required, "用户参数是可选的");
    assert_eq!(args.flags.len(), 1);
    assert_eq!(args.flags[0].name, "top");
    assert_eq!(args.flags[0].default.as_deref(), Some("10"));
    let registry = CommandRegistry::new();
    registry.register_plugin(example_rating::plugin(store()));
    let names = registry.names();
    assert!(names.contains(&COMMAND.to_string()), "{names:?}");
    assert!(
        names.contains(&"best30".to_string()),
        "别名要注册: {names:?}"
    );
}

#[test]
fn average_rating_takes_the_best_scores_first() {
    let scores = vec![
        Score::new("a", "F", 1.0),
        Score::new("b", "F", 3.0),
        Score::new("c", "F", 2.0),
    ];
    assert_eq!(average_rating(&scores, 2), 2.5);
    assert_eq!(average_rating(&scores, 3), 2.0);
    assert_eq!(average_rating(&scores, 0), 0.0);
    assert_eq!(average_rating(&[], 5), 0.0);
}

#[tokio::test]
async fn dispatch_returns_a_render_card_and_a_keyboard() {
    let outcome = router(store())
        .dispatch_detailed(incoming("/b30 alice --top=5", "u1"))
        .await;
    let reply = outcome.reply.expect("必须有回复");
    assert_eq!(reply.kind, ReplyKind::Handler);
    assert!(reply.has_render(), "回复要带渲染卡片");
    let body = reply.body();
    let card = card_of(&reply);
    assert_eq!(card.request.width, CARD_WIDTH);
    assert_eq!(card.request.height, CARD_HEIGHT);
    assert_eq!(card.request.scale, CARD_SCALE);
    assert_eq!(card.request.physical_size(), physical_size_i32());
    assert_eq!(card.label, "b30.png");
    let keyboard = reply.keyboard.expect("回复要带按钮板");
    let json = keyboard.to_json().expect("键盘可序列化");
    let parsed: serde_json::Value = serde_json::from_slice(&json).expect("键盘 JSON");
    assert_eq!(
        parsed["content"]["rows"][0]["buttons"][0]["id"],
        serde_json::json!("rating.refresh")
    );
    assert!(body.contains("Best5"), "文本要说明查询范围: {body}");
}

fn physical_size_i32() -> (i32, i32) {
    (physical_size().0 as i32, physical_size().1 as i32)
}

#[tokio::test]
async fn unknown_user_gets_a_text_hint_instead_of_a_card() {
    let outcome = router(store())
        .dispatch_detailed(incoming("/b30 nobody", "u1"))
        .await;
    let reply = outcome.reply.expect("必须有回复");
    assert!(!reply.has_render(), "没有成绩就不该画图");
    assert!(reply.keyboard.is_none());
    assert!(reply.body().contains("nobody"), "{}", reply.body());
}

#[tokio::test]
async fn missing_user_argument_falls_back_to_the_caller() {
    let store = Arc::new(MemoryStore::new());
    store.seed("u1", vec![Score::new("自选", "F", 12.0)]);
    let outcome = router(store)
        .dispatch_detailed(incoming("/b30", "u1"))
        .await;
    let reply = outcome.reply.expect("必须有回复");
    assert!(reply.has_render(), "省略用户时应查自己");
    assert!(reply.body().contains("u1"), "{}", reply.body());
}

#[tokio::test]
async fn too_small_top_is_clamped_instead_of_panicking() {
    let outcome = router(store())
        .dispatch_detailed(incoming("/b30 alice --top=0", "u1"))
        .await;
    let reply = outcome.reply.expect("必须有回复");
    assert!(reply.body().contains("Best1"), "top 夹到 1: {}", reply.body());
}

#[tokio::test]
async fn card_painter_produces_a_real_png() {
    let bytes = render_card_png().await;
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "必须是 PNG");
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("宽"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("高"));
    let (expected_width, expected_height) = physical_size();
    assert_eq!(width as usize, expected_width, "HiDPI 物理宽度");
    assert_eq!(height as usize, expected_height, "HiDPI 物理高度");
    assert!(
        bytes.len() > 150_000,
        "高分辨率卡片图不该是空白: {} 字节",
        bytes.len()
    );
    if let Ok(dir) = std::env::var("AURORIX_EXAMPLE_OUT") {
        let path = std::path::Path::new(&dir).join("b30.png");
        std::fs::write(&path, &bytes).expect("写出示例图");
        println!("示例卡片已写入 {}", path.display());
    }
}

#[tokio::test]
async fn painter_draws_visible_pixels() {
    let outcome = router(store())
        .dispatch_detailed(incoming("/b30 alice", "u1"))
        .await;
    let reply = outcome.reply.expect("回复");
    let card = card_of(&reply).clone();
    let mut surface =
        aurorix_render::skia_safe::surfaces::raster_n32_premul((CARD_WIDTH, CARD_HEIGHT)).unwrap();
    let backend = SkiaBackend::new();
    let context = RenderContext::new(Arc::new(backend), 16, 16 * 1024 * 1024);
    let kit = CanvasKit::new(surface.canvas(), &context);
    (card.painter)(&kit);
    let image = surface.image_snapshot();
    let pixmap = image.peek_pixels().expect("可读像素");
    let pixels = pixmap.bytes().expect("像素字节");
    let distinct = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| (pixel[0], pixel[1], pixel[2]))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        distinct.len() > 8,
        "卡片必须有多种颜色, 实际 {}",
        distinct.len()
    );
    let offset = (48usize * pixmap.row_bytes()) + (5 * 4);
    let backdrop = &pixels[offset..offset + 3];
    assert!(
        backdrop.iter().all(|channel| *channel < 90),
        "顶部渐变条下方应是深色背景, 实际 {backdrop:?}"
    );
}

#[tokio::test]
async fn rendered_png_reloads_with_expected_bands() {
    let bytes = render_card_png().await;
    let image = RgbaImage::decode(&bytes).expect("PNG 必须能解码");
    let (expected_width, expected_height) = physical_size();
    assert_eq!(
        (image.width, image.height),
        (expected_width, expected_height)
    );
    let scale = CARD_SCALE as usize;
    let sample = |x: usize, y: usize| {
        let pixel = image.pixel(x * scale, y * scale);
        (pixel[0], pixel[1], pixel[2], pixel[3])
    };
    let bright = |x: usize, y: usize| {
        let (first, second, third, _) = sample(x, y);
        first as u32 + second as u32 + third as u32
    };
    let accent = sample(450, 3);
    assert!(
        accent.2 > accent.0 && accent.2 > 120 && accent.0 > 60,
        "顶部渐变条应为蓝紫色, 实际 {accent:?}"
    );
    let backdrop = sample(20, 700);
    assert!(
        backdrop.0 < 90 && backdrop.1 < 90 && backdrop.2 < 90,
        "卡片区左侧留白应是深背景, 实际 {backdrop:?}"
    );
    let header_row = (60..110).map(|y| bright(60, y)).max().unwrap_or(0);
    assert!(header_row > 300, "标题区必须有亮色文字, 实际 {header_row}");
    let title_row = (300..420)
        .flat_map(|y| (120..760).map(move |x| (x, y)))
        .map(|(x, y)| bright(x, y))
        .max()
        .unwrap_or(0);
    assert!(title_row > 300, "曲名区必须有亮色文字, 实际 {title_row}");
    let footer_row = (1050..1180)
        .flat_map(|y| (60..840).map(move |x| (x, y)))
        .map(|(x, y)| bright(x, y))
        .max()
        .unwrap_or(0);
    assert!(footer_row > 200, "底部说明必须有文字, 实际 {footer_row}");
    let panel = sample(150, 260);
    assert!(
        panel.2 > panel.0 && panel.2 >= 40,
        "成绩卡面板应与紫色难度块形成冷暖对比, 实际 {panel:?}"
    );
}

#[tokio::test]
async fn png_proves_gradients_shadows_and_crisp_text() {
    let bytes = render_card_png().await;
    let image = RgbaImage::decode(&bytes).expect("PNG 必须能解码");
    let analysis = analyze(&image, CARD_SCALE);
    println!(
        "独立颜色 {} · 文字带 方差 {:.1} delta {:.3} p99 {:.1} · 背景带 方差 {:.1} delta {:.3} p99 {:.1} · 难度块 色度 {} 饱和像素 {}",
        analysis.distinct_colors,
        analysis.text_band.variance,
        analysis.text_band.edge_delta,
        analysis.text_band.edge_peak,
        analysis.background_band.variance,
        analysis.background_band.edge_delta,
        analysis.background_band.edge_peak,
        analysis.pill_max_chroma,
        analysis.pill_saturated_pixels
    );

    assert!(
        analysis.distinct_colors > 2000,
        "渐变 / 投影 / 抗锯齿应产生大量独立颜色, 实际 {}",
        analysis.distinct_colors
    );

    let text = analysis.text_band;
    let background = analysis.background_band;
    assert!(
        background.variance < 200.0,
        "纯背景带必须平滑, 实际方差 {:.1}",
        background.variance
    );
    assert!(
        text.variance > 1000.0,
        "文字带必须有强烈明暗对比, 实际方差 {:.1}",
        text.variance
    );
    assert!(
        text.variance > background.variance * 20.0,
        "文字带方差 {:.1} 应远高于背景带 {:.1}",
        text.variance,
        background.variance
    );
    assert!(
        text.max > 200.0,
        "文字带必须有接近白色的字形, 实际最亮 {:.1}",
        text.max
    );

    assert!(
        text.edge_delta > 3.0,
        "文字带相邻像素平均亮度差应显著, 实际 {:.3}",
        text.edge_delta
    );
    assert!(
        text.edge_delta > background.edge_delta * 50.0,
        "文字带边缘差 {:.3} 应远高于背景带 {:.3}",
        text.edge_delta,
        background.edge_delta
    );
    assert!(
        text.edge_peak > 100.0,
        "字形边缘必须是硬跳变 (2 倍缩放), 实际 p99 {:.1}",
        text.edge_peak
    );
    assert!(
        background.edge_peak < 2.0,
        "背景带不该有硬边, 实际 p99 {:.1}",
        background.edge_peak
    );

    assert!(
        analysis.pill_max_chroma > 90,
        "难度色块必须有高饱和度, 实际最大色度 {}",
        analysis.pill_max_chroma
    );
    assert!(
        analysis.pill_saturated_pixels > 1000,
        "难度色块需要足够面积的高饱和像素, 实际 {}",
        analysis.pill_saturated_pixels
    );
    assert_eq!(analysis.top_colors.len(), 5, "调色板统计应给出前 5 色");
    let dominant = analysis.top_colors[0].0;
    assert!(
        dominant[2] > dominant[0] && dominant[2] > 35,
        "出现最多的颜色应是偏蓝的深色面板, 实际 {dominant:?}"
    );
}

#[test]
fn cjk_fallback_yields_full_width_glyphs() {
    let mut surface =
        aurorix_render::skia_safe::surfaces::raster_n32_premul((64, 64)).expect("画布");
    let backend = SkiaBackend::new();
    let context = RenderContext::new(Arc::new(backend), 16, 4 * 1024 * 1024);
    let kit = CanvasKit::with_chain(surface.canvas(), &context, card_font_chain());
    let style = TextStyle::new(FONT_SANS, 40).with_color(Color::WHITE);
    let cjk = kit.measure("世界の終わり", &style).expect("可测量日文");
    let latin = kit.measure("abcdef", &style).expect("可测量拉丁字母");
    assert!(
        cjk.width > 40.0 * 5.5,
        "CJK 回退字体应给出全宽字形, 实际宽度 {:.1}",
        cjk.width
    );
    assert!(
        latin.width < cjk.width * 0.7,
        "拉丁字母宽度 {:.1} 应明显小于日文 {:.1}",
        latin.width,
        cjk.width
    );
    let drawn = kit
        .draw_text("世界の終わり", 4.0, 40.0, &style)
        .expect("可绘制日文");
    assert!((drawn.width - cjk.width).abs() < 0.5);
    assert_eq!(surface.image_info().width(), 64);
}

#[test]
fn docs_are_only_written_when_the_sample_env_is_set() {
    let (png, webp) = sample_output_paths(Path::new("docs"));
    assert_eq!(png, Path::new("docs").join(SAMPLE_PNG_NAME));
    assert_eq!(webp, Path::new("docs").join(SAMPLE_WEBP_NAME));
    assert!(sample_output_dir_from(None).is_none());
    assert!(sample_output_dir_from(Some(OsStr::new(""))).is_none());
    assert_eq!(
        sample_output_dir_from(Some(OsStr::new("docs"))),
        Some(PathBuf::from("docs"))
    );

    let docs = docs_dir();
    let before = snapshot(&docs);
    let gated = emit_sample_outputs(None, b"png-payload", b"webp-payload").expect("默认不写盘");
    assert!(gated.is_none(), "没有环境变量时不得写出示例图");
    let after = snapshot(&docs);
    assert_eq!(
        before, after,
        "AURORIX_SAMPLE_OUT 未设置时 docs/ 不允许被改动"
    );

    let temp = std::env::temp_dir().join(format!("aurorix-sample-{}", std::process::id()));
    let written = emit_sample_outputs(Some(&temp), b"png-payload", b"webp-payload")
        .expect("临时目录可写")
        .expect("有目录时才写");
    assert_eq!(written.png, temp.join(SAMPLE_PNG_NAME));
    assert_eq!(
        std::fs::read(&written.png).expect("读回 png"),
        b"png-payload"
    );
    assert_eq!(
        std::fs::read(&written.webp).expect("读回 webp"),
        b"webp-payload"
    );
    std::fs::remove_dir_all(&temp).expect("清理临时目录");
}

#[test]
fn default_request_is_hidpi_and_opaque() {
    let request = RenderRequest::new(CARD_WIDTH, CARD_HEIGHT)
        .with_scale(CARD_SCALE)
        .with_background(Color::rgb(10, 12, 22));
    assert_eq!(request.physical_size(), physical_size_i32());
    assert_eq!(request.estimated_bytes(), 1800 * 2400 * 4);
    assert_eq!(request.effective_background(), Some(Color::rgb(10, 12, 22)));
}
