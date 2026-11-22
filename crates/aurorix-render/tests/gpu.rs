use aurorix_render::gpu::{
    self, GpuDeviceKind, GpuInfo, GpuMode, GpuRuntimeState, GpuSupport, GPU_FEATURE_ENABLED,
};
use aurorix_render::{Color, RenderContext, RenderRequest, SkiaBackend};
use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};
use std::sync::Mutex;

static GLOBAL_STATE: Mutex<()> = Mutex::new(());

fn lock_global() -> std::sync::MutexGuard<'static, ()> {
    GLOBAL_STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn render_shapes() -> Vec<u8> {
    let context = RenderContext::new(SkiaBackend::handle(), 8, 1024 * 1024);
    let request = RenderRequest::new(160, 160).with_background(Color::rgb(18, 24, 32));
    let bytes = context
        .draw(request, |canvas, _context| {
            let mut paint = skia_safe::Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(skia_safe::Color::from_argb(255, 220, 90, 40));
            canvas.draw_circle((62.0, 62.0), 38.0, &paint);
            paint.set_color(skia_safe::Color::from_argb(160, 60, 150, 240));
            canvas.draw_circle((96.0, 96.0), 46.0, &paint);
            aurorix_render::fill_rect(canvas, 12.0, 120.0, 60.0, 24.0, Color::WHITE);
        })
        .expect("形状画布应当可以渲染");
    rgba_of(&bytes, 160)
}

fn rgba_of(bytes: &[u8], side: i32) -> Vec<u8> {
    let image = Image::from_encoded(Data::new_copy(bytes)).expect("编码结果应当可以解码");
    let info = ImageInfo::new((side, side), ColorType::RGBA8888, AlphaType::Premul, None);
    let row_bytes = info.min_row_bytes();
    let mut pixels = vec![0u8; info.compute_byte_size(row_bytes)];
    assert!(image.read_pixels(
        &info,
        &mut pixels,
        row_bytes,
        (0, 0),
        skia_safe::image::CachingHint::Allow
    ));
    pixels
}

fn channel_stats(left: &[u8], right: &[u8]) -> (u8, f64, f64) {
    let mut worst = 0u8;
    let mut total = 0u64;
    let mut differing = 0usize;
    for (a, b) in left.iter().zip(right.iter()) {
        let delta = a.abs_diff(*b);
        worst = worst.max(delta);
        total += u64::from(delta);
        if delta > 0 {
            differing += 1;
        }
    }
    let count = left.len().max(1) as f64;
    (worst, total as f64 / count, differing as f64 / count)
}

fn solid_pixel(bytes: &[u8], side: i32) -> Vec<u8> {
    let image = Image::from_encoded(Data::new_copy(bytes)).expect("编码结果应当可以解码");
    let info = ImageInfo::new((side, side), ColorType::RGBA8888, AlphaType::Premul, None);
    let row_bytes = info.min_row_bytes();
    let mut pixels = vec![0u8; info.compute_byte_size(row_bytes)];
    assert!(
        image.read_pixels(
            &info,
            &mut pixels,
            row_bytes,
            (0, 0),
            skia_safe::image::CachingHint::Allow
        ),
        "解码后的图片应当可以回读像素"
    );
    pixels[0..4].to_vec()
}

#[test]
fn mode_parses_documented_spellings() {
    assert_eq!(GpuMode::parse("auto"), Some(GpuMode::Auto));
    assert_eq!(GpuMode::parse(" AUTO "), Some(GpuMode::Auto));
    assert_eq!(GpuMode::parse("default"), Some(GpuMode::Auto));
    assert_eq!(GpuMode::parse("on"), Some(GpuMode::On));
    assert_eq!(GpuMode::parse("force"), Some(GpuMode::On));
    assert_eq!(GpuMode::parse("1"), Some(GpuMode::On));
    assert_eq!(GpuMode::parse("off"), Some(GpuMode::Off));
    assert_eq!(GpuMode::parse("disabled"), Some(GpuMode::Off));
    assert_eq!(GpuMode::parse("0"), Some(GpuMode::Off));
    assert_eq!(GpuMode::parse("maybe"), None);
    assert_eq!(GpuMode::default(), GpuMode::Auto);
    assert_eq!(GpuMode::Auto.as_str(), "auto");
    assert_eq!(GpuMode::On.as_str(), "on");
    assert_eq!(GpuMode::Off.as_str(), "off");
}

#[test]
fn env_name_is_stable() {
    assert_eq!(GPU_FEATURE_ENABLED, cfg!(feature = "gpu"));
}

#[test]
fn disabled_mode_never_touches_the_gpu() {
    let support = gpu::probe_with(GpuMode::Off);
    assert!(support.is_disabled(), "显式关闭时应当报告已关闭");
    assert!(!support.is_available());
    assert!(support.info().is_none());
    assert!(support.reason().is_none());
    assert!(
        support.describe().contains("CPU"),
        "关闭 GPU 的说明应当点明走 CPU 光栅"
    );
}

#[test]
fn unavailable_probe_explains_the_reason() {
    let support = gpu::probe();
    match &support {
        GpuSupport::Available(info) => {
            assert!(!info.device_name.is_empty(), "GPU 名称不应为空");
            assert!(!info.label().is_empty());
            assert!(
                support.describe().contains("GPU"),
                "可用时的说明应当提到 GPU"
            );
        }
        GpuSupport::Unavailable(reason) => {
            assert!(
                !reason.trim().is_empty(),
                "不可用时必须给出可读原因, 而不是空字符串"
            );
            assert!(support.describe().contains("CPU"));
        }
        GpuSupport::Disabled => {
            assert!(
                GpuMode::from_env() == GpuMode::Off,
                "只有显式关闭才会报告 Disabled"
            );
        }
    }
    if !GPU_FEATURE_ENABLED {
        let support = gpu::probe_with(GpuMode::On);
        assert!(
            matches!(support, GpuSupport::Unavailable(ref reason) if reason.contains("gpu 特性")),
            "未启用 gpu 特性时必须说明是构建期原因"
        );
    }
}

#[test]
fn decode_version_splits_vulkan_components() {
    let raw = (1 << 22) | (3 << 12) | 275;
    assert_eq!(gpu::decode_version(raw), (1, 3, 275));
    assert_eq!(gpu::decode_version(0), (0, 0, 0));
    assert_eq!(gpu::decode_version(u32::MAX), (0x7f, 0x3ff, 0xfff));
}

#[test]
fn device_kind_labels_are_stable() {
    assert_eq!(GpuDeviceKind::Integrated.label(), "集成显卡");
    assert_eq!(GpuDeviceKind::Discrete.label(), "独立显卡");
    assert_eq!(GpuDeviceKind::Virtual.label(), "虚拟显卡");
    assert_eq!(GpuDeviceKind::Cpu.label(), "软件设备");
    assert_eq!(GpuDeviceKind::Other.label(), "其它设备");
}

#[test]
fn info_renders_api_version_text() {
    let info = GpuInfo {
        api: "vulkan",
        device_name: String::from("Test GPU"),
        kind: GpuDeviceKind::Discrete,
        vendor_id: 0x10de,
        device_id: 0x2684,
        driver_version: (1, 2, 3),
        api_version: (1, 3, 250),
    };
    assert_eq!(info.label(), "vulkan (Test GPU)");
    assert_eq!(info.api_version_text(), "1.3.250");
}

#[test]
fn failure_marks_runtime_as_raster_and_reset_clears_it() {
    let _guard = lock_global();
    gpu::reset_failure();
    let before = gpu::runtime_state();
    assert!(!before.failed);
    assert_eq!(before.fallbacks, 0);

    gpu::note_failure("模拟失败");
    let after = gpu::runtime_state();
    assert!(after.failed, "运行期失败后必须标记为回退状态");
    assert!(after.failure.is_some(), "回退状态必须留下可读原因");
    assert!(after.fallbacks >= 1);
    assert!(
        !after.hardware_accelerated(),
        "回退状态下不应再声明硬件加速"
    );
    assert!(
        after.describe().contains("CPU"),
        "回退状态下应当说明走 CPU 光栅"
    );

    gpu::note_failure("再次失败");
    assert!(
        gpu::fallback_count() >= 2,
        "每次失败都要计入回退计数, 便于观测"
    );

    assert!(gpu::reset_failure(), "重置应当报告之前确实处于失败状态");
    let restored = gpu::runtime_state();
    assert!(!restored.failed);
    assert_eq!(restored.fallbacks, 0);
    assert!(!gpu::reset_failure(), "重复重置应当报告无需重置");
}

#[test]
fn runtime_state_reports_probe_support() {
    let _guard = lock_global();
    let state: GpuRuntimeState = gpu::runtime_state();
    assert_eq!(state.support, *gpu::cached());
    assert_eq!(
        state.hardware_accelerated(),
        state.support.is_available() && !state.failed
    );
}

#[test]
fn solid_background_survives_whichever_backend_draws_it() {
    let context = RenderContext::new(SkiaBackend::handle(), 8, 1024 * 1024);
    let request = RenderRequest::new(64, 64).with_background(Color::rgb(10, 20, 30));
    let bytes = context
        .draw(request, |_canvas, _context| {})
        .expect("纯色画布应当可以渲染");
    assert_eq!(
        solid_pixel(&bytes, 64),
        vec![10, 20, 30, 255],
        "无论走 GPU 还是 CPU 光栅, 纯色背景都必须一致"
    );
}

#[test]
fn gpu_output_matches_raster_output() {
    let _guard = lock_global();
    gpu::reset_failure();
    let accelerated = gpu::hardware_accelerated();
    let accelerated_pixels = render_shapes();
    gpu::note_failure("像素等价性对照");
    assert!(
        !gpu::hardware_accelerated(),
        "标记失败后必须切到光栅路径, 否则对照没有意义"
    );
    let raster_pixels = render_shapes();
    gpu::reset_failure();
    assert_eq!(
        accelerated_pixels.len(),
        raster_pixels.len(),
        "两条路径产出的画布尺寸必须一致"
    );
    let (worst, mean, ratio) = channel_stats(&accelerated_pixels, &raster_pixels);
    if accelerated {
        assert!(
            worst <= 128,
            "GPU 与光栅的单通道最大差异应只出现在抗锯齿边缘, 实测 {worst}"
        );
        assert!(
            mean < 1.0,
            "GPU 与光栅的平均通道差异应当接近 0, 实测 {mean:.4}"
        );
        assert!(
            ratio < 0.08,
            "差异像素占比应当只覆盖形状边缘, 实测 {ratio:.4}"
        );
    } else {
        assert_eq!(worst, 0, "同一条光栅路径两次渲染必须逐像素一致");
    }
}
