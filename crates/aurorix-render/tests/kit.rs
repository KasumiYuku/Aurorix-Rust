use std::sync::Arc;

use aurorix_render::{
    Border, CanvasKit, Color, FitMode, FontChain, GradientStop, LinearGradient, Padding,
    RadialGradient, RenderContext, SkiaBackend, TextAlign, TextStyle,
};
use skia_safe::{
    surfaces, AlphaType, BlendMode, ColorType, Data, Image, ImageInfo, Rect,
};

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

    fn alpha(&self, x: i32, y: i32) -> u8 {
        self.pixel(x, y).3
    }

    fn painted(&self) -> usize {
        self.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] > 0)
            .count()
    }
}

fn context() -> Arc<RenderContext> {
    RenderContext::new(Arc::new(SkiaBackend::new()), 32, 16 * 1024 * 1024)
}

fn render(width: i32, height: i32, paint: impl FnOnce(&CanvasKit)) -> Frame {
    let context = context();
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
        let kit = CanvasKit::new(canvas, &context);
        paint(&kit);
    }
    Frame {
        width,
        height,
        row,
        pixels,
    }
}

fn cover_image() -> Image {
    let info = ImageInfo::new((64, 64), ColorType::RGBA8888, AlphaType::Premul, None);
    let row = info.min_row_bytes();
    let mut buffer = vec![0u8; info.compute_byte_size(row)];
    {
        let mut surface = surfaces::wrap_pixels(&info, &mut buffer, row, None).expect("贴图画布");
        surface
            .canvas()
            .clear(skia_safe::Color::from_argb(255, 220, 120, 40));
    }
    skia_safe::images::raster_from_data(&info, Data::new_copy(&buffer), row)
        .expect("贴图应当可构造")
}

#[test]
fn rrect_fill_keeps_corners_transparent() {
    let frame = render(120, 120, |kit| {
        kit.rrect_fill(
            Rect::from_xywh(10.0, 10.0, 100.0, 100.0),
            24.0,
            Color::rgb(240, 40, 40),
        );
    });
    assert_eq!(frame.alpha(10, 10), 0, "圆角外角应当完全透明");
    assert_eq!(frame.alpha(109, 109), 0, "右下圆角外角应当完全透明");
    let center = frame.pixel(60, 60);
    assert_eq!(center.3, 255, "中心应当完全不透明");
    assert!(center.0 > 200, "中心应当是填充色");
}

#[test]
fn linear_gradient_ends_differ() {
    let gradient = LinearGradient::new([
        GradientStop::new(0.0, Color::rgb(255, 0, 0)),
        GradientStop::new(1.0, Color::rgb(0, 0, 255)),
    ]);
    let frame = render(120, 40, |kit| {
        kit.fill_gradient(
            Rect::from_xywh(0.0, 0.0, 120.0, 40.0),
            0.0,
            &gradient.into(),
        )
        .expect("线性渐变应当可绘制");
    });
    let left = frame.pixel(1, 20);
    let right = frame.pixel(118, 20);
    assert!(left.0 > 200 && left.2 < 60, "左端应当接近起始色: {left:?}");
    assert!(
        right.2 > 200 && right.0 < 60,
        "右端应当接近结束色: {right:?}"
    );
}

#[test]
fn linear_gradient_angle_and_stops_are_supported() {
    let gradient = LinearGradient::new([
        GradientStop::new(0.0, Color::rgb(0, 255, 0)),
        GradientStop::new(0.5, Color::rgb(255, 255, 0)),
        GradientStop::new(1.0, Color::rgb(0, 0, 0)),
    ])
    .with_angle(90.0);
    let frame = render(40, 120, |kit| {
        kit.fill_gradient(
            Rect::from_xywh(0.0, 0.0, 40.0, 120.0),
            0.0,
            &gradient.into(),
        )
        .expect("角度渐变应当可绘制");
    });
    let top = frame.pixel(20, 2);
    let bottom = frame.pixel(20, 117);
    assert!(top.1 > bottom.1, "角度渐变应当沿垂直方向变化");
    assert!(bottom.0 < 60 && bottom.1 < 60, "底部应当接近结束色");
}

#[test]
fn radial_gradient_center_differs_from_edge() {
    let gradient = RadialGradient::new([
        GradientStop::new(0.0, Color::rgb(255, 255, 255)),
        GradientStop::new(1.0, Color::rgb(10, 10, 10)),
    ]);
    let frame = render(120, 120, |kit| {
        kit.fill_gradient(
            Rect::from_xywh(0.0, 0.0, 120.0, 120.0),
            60.0,
            &gradient.into(),
        )
        .expect("径向渐变应当可绘制");
    });
    let center = frame.pixel(60, 60);
    let edge = frame.pixel(1, 60);
    assert!(
        center.0 > edge.0,
        "中心应当比边缘更亮: {center:?} vs {edge:?}"
    );
    assert!(center.0 > 180, "中心应当接近起始色");
}

#[test]
fn pill_draws_gradient_and_border() {
    let gradient = LinearGradient::new([
        GradientStop::new(0.0, Color::rgb(30, 120, 240)),
        GradientStop::new(1.0, Color::rgb(10, 40, 120)),
    ]);
    let frame = render(160, 60, |kit| {
        kit.pill(
            Rect::from_xywh(10.0, 10.0, 140.0, 40.0),
            &gradient.into(),
            Some(Border::new(Color::rgb(255, 255, 255), 3.0)),
        )
        .expect("胶囊应当可绘制");
    });
    assert_eq!(frame.alpha(9, 30), 0, "胶囊左外侧应当透明");
    assert_eq!(frame.alpha(80, 30), 255, "胶囊中心应当不透明");
    let border = frame.pixel(80, 12);
    assert!(border.0 > 180, "边框应当是白色: {border:?}");
}

#[test]
fn blur_layer_averages_neighbouring_pixels() {
    let frame = render(160, 160, |kit| {
        kit.blur_layer(6.0, |kit| {
            kit.fill_rect(
                Rect::from_xywh(50.0, 50.0, 60.0, 60.0),
                Color::rgb(255, 255, 255),
            );
        });
    });
    assert_eq!(frame.alpha(80, 80), 255, "被模糊区域中心仍然不透明");
    let near_edge = frame.alpha(112, 80);
    assert!(
        near_edge > 0 && near_edge < 255,
        "模糊后边缘像素应当被平均成半透明: {near_edge}"
    );
    assert_eq!(frame.alpha(4, 4), 0, "远处像素不应被模糊污染");
}

#[test]
fn drop_shadow_paints_outside_the_shape() {
    let frame = render(160, 160, |kit| {
        kit.drop_shadow(Color::rgb(0, 0, 0), 18.0, 18.0, 6.0, |kit| {
            kit.rrect_fill(
                Rect::from_xywh(30.0, 30.0, 60.0, 60.0),
                12.0,
                Color::rgb(255, 255, 255),
            );
        });
    });
    assert_eq!(frame.alpha(60, 60), 255, "本体应当不透明");
    let shadow = frame.alpha(95, 95);
    assert!(shadow > 0, "阴影应当绘制到本体之外: {shadow}");
    assert!(shadow < 255, "阴影应当带有模糊过渡: {shadow}");
}

#[test]
fn rounded_image_clips_all_four_corners() {
    let image = cover_image();
    let frame = render(120, 120, |kit| {
        kit.rounded_image(&image, Rect::from_xywh(0.0, 0.0, 120.0, 120.0), 28.0);
    });
    assert_eq!(frame.alpha(0, 0), 0, "左上角应当被裁掉");
    assert_eq!(frame.alpha(119, 0), 0, "右上角应当被裁掉");
    assert_eq!(frame.alpha(0, 119), 0, "左下角应当被裁掉");
    assert_eq!(frame.alpha(119, 119), 0, "右下角应当被裁掉");
    let center = frame.pixel(60, 60);
    assert_eq!(center.3, 255, "中心应当完整贴图");
    assert!(center.0 > 180, "中心应当是贴图颜色");
}

#[test]
fn layer_multiply_darkens_content() {
    let plain = render(60, 60, |kit| {
        kit.fill_rect(
            Rect::from_xywh(0.0, 0.0, 60.0, 60.0),
            Color::rgb(200, 200, 200),
        );
    });
    let blended = render(60, 60, |kit| {
        kit.fill_rect(
            Rect::from_xywh(0.0, 0.0, 60.0, 60.0),
            Color::rgb(200, 200, 200),
        );
        kit.layer(
            Rect::from_xywh(0.0, 0.0, 60.0, 60.0),
            1.0,
            BlendMode::Multiply,
            |kit| {
                kit.fill_rect(
                    Rect::from_xywh(0.0, 0.0, 60.0, 60.0),
                    Color::rgb(128, 128, 128),
                );
            },
        );
    });
    let base = plain.pixel(30, 30);
    let mixed = blended.pixel(30, 30);
    assert!(
        mixed.0 < base.0,
        "Multiply 图层应当压暗下层: {mixed:?} vs {base:?}"
    );
}

#[test]
fn layer_alpha_scales_content() {
    let frame = render(60, 60, |kit| {
        kit.layer(
            Rect::from_xywh(0.0, 0.0, 60.0, 60.0),
            0.5,
            BlendMode::SrcOver,
            |kit| {
                kit.fill_rect(Rect::from_xywh(0.0, 0.0, 60.0, 60.0), Color::rgb(255, 0, 0));
            },
        );
    });
    let alpha = frame.alpha(30, 30);
    assert!(
        (120..160).contains(&alpha),
        "半透明图层应当产生中间透明度: {alpha}"
    );
}

#[test]
fn circle_and_stroke_draw_pixels() {
    let frame = render(100, 100, |kit| {
        kit.circle((50.0, 50.0), 30.0, Color::rgb(255, 128, 0));
        kit.stroke(
            Rect::from_xywh(10.0, 10.0, 80.0, 80.0),
            8.0,
            Border::new(Color::rgb(0, 0, 255), 4.0),
        );
    });
    assert_eq!(frame.pixel(50, 50).3, 255, "圆心应当被填充");
    let edge = frame.pixel(50, 11);
    assert!(edge.2 > 150, "描边应当绘制在矩形边界上: {edge:?}");
}

#[test]
fn badge_is_wider_than_its_text() {
    let style = TextStyle::new("sans-serif", 24).with_color(Color::BLACK);
    let mut width = 0.0f32;
    let mut badge_width = 0.0f32;
    let frame = render(300, 80, |kit| {
        width = kit
            .measure("SSS+ 1,234,567", &style)
            .expect("测量应当成功")
            .width;
        let rect = kit
            .badge_at(
                skia_safe::Point::new(8.0, 10.0),
                "SSS+ 1,234,567",
                &style,
                Color::rgb(255, 190, 40),
                Color::BLACK,
                Padding::new(14.0, 6.0),
            )
            .expect("徽章应当可绘制");
        badge_width = rect.width();
    });
    assert!(width > 0.0, "文本宽度应当为正");
    assert!(
        badge_width >= width,
        "徽章宽度应当不小于文本宽度: {badge_width} vs {width}"
    );
    assert_eq!(frame.alpha(10, 40), 255, "徽章背景应当被填充");
}

#[test]
fn fit_to_width_never_exceeds_max_width() {
    let style = TextStyle::new("sans-serif", 28);
    let text = "Aurorix 音游成绩图超级长的一段标题";
    let mut measured = 0.0f32;
    let mut truncated = String::new();
    let mut narrowed = TextStyle::default();
    let frame = render(400, 100, |kit| {
        let full = kit.measure(text, &style).expect("测量应当成功").width;
        assert!(full > 180.0, "原始文本应当超宽: {full}");
        let (fitted, fitted_style) = kit.fit_to_width(text, &style, 180.0);
        truncated = fitted;
        narrowed = fitted_style;
        measured = kit
            .measure(&truncated, &narrowed)
            .expect("测量应当成功")
            .width;
        kit.draw_text(&truncated, 4.0, 48.0, &narrowed)
            .expect("绘制应当成功");
    });
    assert!(
        measured <= 180.5,
        "fit_to_width 结果宽度应当不超过上限: {measured}"
    );
    assert!(!truncated.is_empty(), "结果不应为空");
    assert!(frame.painted() > 0, "结果应当被真正绘制出来");
}

#[test]
fn fit_modes_are_selectable() {
    let style = TextStyle::new("sans-serif", 30);
    render(400, 100, |kit| {
        let (truncated, truncate_style) =
            kit.fit_to_width_with(FitMode::Truncate, "Aurorix 很长很长的标题", &style, 150.0);
        assert!(truncate_style.size == style.size, "截断模式不应改变字号");
        assert!(
            truncated.ends_with('…'),
            "截断模式结果应当以省略号结尾: {truncated}"
        );

        let (shrunk, shrink_style) =
            kit.fit_to_width_with(FitMode::Shrink, "Aurorix 很长很长的标题", &style, 150.0);
        assert!(shrink_style.size < style.size, "缩放模式应当缩小字号");
        assert_eq!(shrunk, "Aurorix 很长很长的标题", "缩放模式不应丢字");
    });
}

#[test]
fn truncate_with_ellipsis_shortens_long_text() {
    let style = TextStyle::new("sans-serif", 26);
    render(400, 100, |kit| {
        let short = kit.truncate_with_ellipsis("Aurorix", &style, 400.0);
        assert_eq!(short, "Aurorix", "足够宽时不应截断");
        let long = kit.truncate_with_ellipsis("Aurorix Aurorix Aurorix", &style, 90.0);
        assert!(long.ends_with('…'), "超宽文本应当以省略号结尾: {long}");
        let width = kit.measure(&long, &style).expect("测量应当成功").width;
        assert!(width <= 90.5, "截断结果不应超过宽度上限: {width}");
    });
}

#[test]
fn fallback_chain_draws_when_primary_family_is_missing() {
    let style = TextStyle::new("Aurorix-NoSuchFamily", 32).with_color(Color::BLACK);
    let chain = FontChain::new("Aurorix-NoSuchFamily")
        .with_fallback("Noto Sans CJK JP")
        .with_fallback("DejaVu Sans");
    let context = context();
    let info = ImageInfo::new((400, 120), ColorType::RGBA8888, AlphaType::Premul, None);
    let row = info.min_row_bytes();
    let mut pixels = vec![0u8; info.compute_byte_size(row)];
    {
        let mut surface =
            surfaces::wrap_pixels(&info, &mut pixels, row, None).expect("画布应当可包装");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::TRANSPARENT);
        let kit = CanvasKit::with_chain(canvas, context.as_ref(), chain);
        let metrics = kit
            .measure("アイウエオ ★♪", &style)
            .expect("回退链应当可以测量日文假名");
        assert!(metrics.width > 0.0, "回退链测量宽度应当大于 0");
        kit.draw_text("アイウエオ ★♪", 6.0, 60.0, &style)
            .expect("回退链应当可以绘制日文假名");
    }
    let frame = Frame {
        width: 400,
        height: 120,
        row,
        pixels,
    };
    assert!(
        frame.painted() > 50,
        "字体缺失时回退链仍应画出非空像素: {}",
        frame.painted()
    );
}

#[test]
fn letter_spacing_and_alignment_take_effect() {
    let tight = TextStyle::new("sans-serif", 24).with_letter_spacing(0.0);
    let loose = TextStyle::new("sans-serif", 24).with_letter_spacing(6.0);
    render(400, 120, |kit| {
        let tight_width = kit.measure("AURORIX", &tight).expect("测量应当成功").width;
        let loose_width = kit.measure("AURORIX", &loose).expect("测量应当成功").width;
        assert!(
            loose_width > tight_width,
            "加大字距后测量宽度应当变大: {loose_width} vs {tight_width}"
        );

        let centered = TextStyle::new("sans-serif", 24).with_align(TextAlign::Center);
        let right = TextStyle::new("sans-serif", 24).with_align(TextAlign::Right);
        let center_metrics = kit
            .draw_text("Aurorix", 200.0, 40.0, &centered)
            .expect("居中绘制应当成功");
        let right_metrics = kit
            .draw_text("Aurorix", 200.0, 90.0, &right)
            .expect("右对齐绘制应当成功");
        assert_eq!(
            center_metrics.width, right_metrics.width,
            "两次测量应当一致"
        );
        assert!(center_metrics.height > 0.0, "文本高度应当为正");
        assert!(center_metrics.baseline > 0.0, "基线距离应当为正");
    });
}
#[test]
fn paragraph_wraps_into_multiple_lines() {
    let style = TextStyle::new("sans-serif", 22);
    let mut lines = 0usize;
    render(300, 300, |kit| {
        let wrapped = kit.wrap(
            "Aurorix 音游成绩图渲染引擎的多行文本换行验证",
            &style,
            160.0,
        );
        lines = wrapped.len();
        assert!(lines > 1, "宽度受限时应当换行, 实际行数 {lines}");
        let metrics = kit
            .draw_paragraph(
                "Aurorix 音游成绩图渲染引擎的多行文本换行验证",
                8.0,
                40.0,
                160.0,
                30.0,
                &style,
            )
            .expect("多行绘制应当成功");
        assert_eq!(metrics, 30.0, "行高应当被透传");
    });
    assert!(lines > 1, "多行排版应当产生多行");
}

#[test]
fn text_layout_errors_are_described_in_chinese() {
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 1024 * 1024);
    let info = ImageInfo::new((40, 40), ColorType::RGBA8888, AlphaType::Premul, None);
    let row = info.min_row_bytes();
    let mut pixels = vec![0u8; info.compute_byte_size(row)];
    let mut surface = surfaces::wrap_pixels(&info, &mut pixels, row, None).expect("画布");
    let kit = CanvasKit::new(surface.canvas(), context.as_ref());
    let style = TextStyle::new("sans-serif", 16);
    let metrics = kit
        .measure("Aurorix", &style)
        .expect("正常文本应当可以测量");
    assert!(metrics.width > 0.0, "正常文本宽度应当为正");
    assert_eq!(metrics.as_tuple().0, metrics.width, "元组视图应当一致");
}

#[test]
fn with_fallback_skips_primary_and_repeats() {
    let chain = FontChain::new("Aurorix-Main")
        .with_fallback("Aurorix-Main")
        .with_fallback("Aurorix-JP")
        .with_fallback("Aurorix-JP")
        .with_fallback("Aurorix-Symbol");
    assert_eq!(chain.primary(), "Aurorix-Main", "主字体不应被改变");
    assert_eq!(chain.fallbacks().len(), 2, "主字体与重复项都不应进入回退链");
    assert_eq!(chain.fallbacks()[0], "Aurorix-JP", "回退顺序应当保持");
    assert_eq!(chain.fallbacks()[1], "Aurorix-Symbol", "回退顺序应当保持");
}

#[test]
fn truncate_with_zero_width_returns_empty() {
    let style = TextStyle::new("DejaVu Sans", 24);
    let frame = render(8, 8, |kit| {
        assert_eq!(
            kit.truncate_with_ellipsis("Aurorix", &style, 0.0),
            "",
            "最大宽度为 0 时必须返回空串"
        );
        assert_eq!(
            kit.truncate_with_ellipsis("Aurorix", &style, -5.0),
            "",
            "负的最大宽度必须返回空串"
        );
    });
    assert!(frame.width > 0, "画布应当正常完成渲染");
}

fn gray_pixels(frame: &Frame) -> usize {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2] && pixel[0] > 0)
        .filter(|pixel| pixel[0] < 255)
        .count()
}

fn half_plane() -> Image {
    let size = 64i32;
    let info = ImageInfo::new((size, size), ColorType::RGBA8888, AlphaType::Premul, None);
    let row = info.min_row_bytes();
    let mut buffer = vec![0u8; info.compute_byte_size(row)];
    {
        let mut surface =
            surfaces::wrap_pixels(&info, &mut buffer, row, None).expect("半平面画布应当可包装");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::BLACK);
        let mut paint = skia_safe::Paint::default();
        paint.set_color(skia_safe::Color::WHITE);
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 33.0, 33.0), &paint);
    }
    skia_safe::images::raster_from_data(&info, Data::new_copy(&buffer), row)
        .expect("半平面贴图应当可构造")
}

#[test]
fn default_sampling_smooths_downscaled_images() {
    let image = half_plane();
    let target = Rect::from_xywh(0.0, 0.0, 40.0, 40.0);
    let smooth = render(40, 40, |kit| {
        kit.image_rect(&image, target);
    });
    let smooth_grays = gray_pixels(&smooth);
    assert!(
        smooth_grays > 0,
        "默认采样在缩小贴图时应当产生过渡灰阶, 实际为 {smooth_grays}"
    );
    let nearest = render(40, 40, |kit| {
        kit.image_rect_with_sampling(&image, target, aurorix_render::sampling_nearest());
    });
    assert_eq!(
        gray_pixels(&nearest),
        0,
        "最近邻采样只应产生原始黑白色, 不应出现过渡灰阶"
    );
}

#[test]
fn crisp_sampling_is_the_default_and_can_be_overridden() {
    assert_eq!(
        aurorix_render::default_sampling(),
        aurorix_render::sampling_cubic(),
        "默认采样应当是 Catmull-Rom 双三次"
    );
    assert!(
        aurorix_render::default_sampling().use_cubic,
        "默认采样必须保留双三次负瓣锐化"
    );
    assert_eq!(
        aurorix_render::default_sampling().mipmap,
        skia_safe::MipmapMode::None,
        "默认采样不得混入 mipmap 低分辨率层"
    );
    assert_ne!(
        aurorix_render::default_sampling(),
        aurorix_render::sampling_nearest(),
        "默认采样绝不能退化成最近邻"
    );
    assert!(
        aurorix_render::sampling_cubic().use_cubic,
        "三次卷积采样应当仍然可用"
    );
    let context = context();
    let mut surface = surfaces::raster_n32_premul((8, 8)).expect("画布应当可分配");
    let kit = CanvasKit::new(surface.canvas(), &context);
    assert_eq!(
        kit.sampling(),
        aurorix_render::default_sampling(),
        "CanvasKit 应当默认使用最高清晰度采样"
    );
    let nearest_kit = CanvasKit::new(surface.canvas(), &context)
        .with_sampling(aurorix_render::sampling_nearest());
    assert_eq!(
        nearest_kit.sampling(),
        aurorix_render::sampling_nearest(),
        "with_sampling 应当可以覆盖默认采样"
    );
}

#[test]
fn cover_image_fills_the_target_rect() {
    let image = cover_image();
    let frame = render(120, 60, |kit| {
        kit.cover_image(&image, Rect::from_xywh(0.0, 0.0, 120.0, 60.0));
    });
    assert_eq!(frame.alpha(1, 1), 255, "cover 贴图应当铺满左上角");
    assert_eq!(frame.alpha(118, 58), 255, "cover 贴图应当铺满右下角");
    let center = frame.pixel(60, 30);
    assert!(center.0 > 180, "cover 贴图中心应当是贴图颜色");
}

#[test]
fn subpixel_positioning_shifts_glyphs_by_a_fraction() {
    let style = TextStyle::new("DejaVu Sans", 28);
    let aligned = render(240, 60, |kit| {
        kit.draw_text("Aurorix", 4.0, 40.0, &style)
            .expect("基准文字应当绘制成功");
    });
    let shifted = render(240, 60, |kit| {
        kit.draw_text("Aurorix", 4.4, 40.0, &style)
            .expect("偏移文字应当绘制成功");
    });
    assert!(aligned.painted() > 0, "基准渲染必须画上字");
    assert_ne!(
        aligned.pixels, shifted.pixels,
        "亚像素定位必须让 0.4px 的水平偏移体现到像素上, 不允许被取整吃掉"
    );
}
