use std::sync::Arc;
use std::time::Duration;

use aurorix_render::{
    fill_rect, skia_safe, Color, RenderContext, RenderError, RenderFormat, RenderRequest,
    SkiaBackend, TextStyle,
};
use skia_safe::{Data, Image};
const GUARD: Duration = Duration::from_secs(60);

fn context() -> Arc<RenderContext> {
    RenderContext::new(Arc::new(SkiaBackend::new()), 8, 8 * 1024 * 1024)
}

fn decode(bytes: &[u8]) -> Image {
    Image::from_encoded(Data::new_copy(bytes)).expect("输出应当可以解码为位图")
}

#[tokio::test]
async fn png_output_has_magic_and_requested_size() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let request = RenderRequest::new(200, 80).with_format(RenderFormat::Png);
        let bytes = context
            .draw(request, |canvas, _context| {
                fill_rect(canvas, 8.0, 8.0, 64.0, 32.0, Color::BLACK);
            })
            .expect("绘制应当成功");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "PNG 魔数应当正确");
        let image = decode(&bytes);
        assert_eq!(image.width(), 200, "回读宽度应当等于请求宽度");
        assert_eq!(image.height(), 80, "回读高度应当等于请求高度");
    })
    .await
    .expect("PNG 输出测试超时");
}

#[tokio::test]
async fn default_output_is_jpeg() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let request = RenderRequest::new(200, 80);
        assert_eq!(request.format, RenderFormat::Jpeg, "默认编码应当是 JPEG");
        let bytes = context
            .draw(request, |canvas, _context| {
                fill_rect(canvas, 8.0, 8.0, 64.0, 32.0, Color::BLACK);
            })
            .expect("绘制应当成功");
        assert!(bytes.starts_with(b"\xff\xd8"), "JPEG 魔数应当正确");
    })
    .await
    .expect("默认编码测试超时");
}

#[tokio::test]
async fn opaque_background_is_applied_for_jpeg() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let request = RenderRequest::new(64, 48).with_format(RenderFormat::Jpeg);
        assert!(
            request.effective_background().is_some(),
            "jpeg 不支持透明时应当自动使用白色底"
        );
        let bytes = context
            .draw(request, |canvas, _context| {
                fill_rect(canvas, 4.0, 4.0, 16.0, 16.0, Color::rgb(200, 40, 40));
            })
            .expect("jpeg 绘制应当成功");
        assert!(bytes.starts_with(&[0xff, 0xd8, 0xff]), "JPEG 魔数应当正确");
        let image = decode(&bytes);
        assert_eq!(image.width(), 64, "jpeg 回读宽度应当一致");
        assert_eq!(image.height(), 48, "jpeg 回读高度应当一致");
    })
    .await
    .expect("jpeg 输出测试超时");
}

#[tokio::test]
async fn png_and_jpeg_encode_with_expected_headers() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        for format in [RenderFormat::Png, RenderFormat::Jpeg] {
            let request = RenderRequest::new(48, 32)
                .with_format(format)
                .with_quality(80);
            let bytes = context
                .draw(request, |canvas, _context| {
                    fill_rect(canvas, 2.0, 2.0, 12.0, 12.0, Color::rgb(10, 120, 220));
                })
                .expect("png 与 jpeg 都应当可以编码");
            assert!(!bytes.is_empty(), "{} 输出不应为空", format.name());
            match format {
                RenderFormat::Png => {
                    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "png 魔数应当正确");
                    assert_eq!(decode(&bytes).width(), 48, "png 回读宽度应当一致");
                }
                RenderFormat::Jpeg => {
                    assert!(bytes.starts_with(&[0xff, 0xd8]), "jpeg 魔数应当正确");
                    assert_eq!(decode(&bytes).height(), 32, "jpeg 回读高度应当一致");
                }
                RenderFormat::WebP => unreachable!("本轮只验证 png 与 jpeg"),
            }
        }
    })
    .await
    .expect("多格式编码测试超时");
}

#[tokio::test]
async fn webp_reports_support_state() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let request = RenderRequest::new(48, 32).with_format(RenderFormat::WebP);
        match context.draw(request, |canvas, _context| {
            fill_rect(canvas, 2.0, 2.0, 12.0, 12.0, Color::rgb(10, 120, 220));
        }) {
            Ok(bytes) => {
                assert!(bytes.starts_with(b"RIFF"), "webp 容器头应当正确");
                assert_eq!(&bytes[8..12], b"WEBP".as_slice(), "webp 标识应当正确");
            }
            Err(RenderError::Backend(message)) => {
                assert!(message.contains("webp"), "缺失 webp 编码时错误应当点明格式");
            }
            Err(other) => panic!("webp 编码失败应当返回后端错误, 实际为 {other:?}"),
        }
    })
    .await
    .expect("webp 支持状态测试超时");
}

#[tokio::test]
async fn text_render_adds_bytes_over_plain_background() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let style = TextStyle::new("sans-serif", 32);
        let width = context
            .measure_text("Aurorix", &style)
            .expect("文本测量应当成功");
        assert!(width > 0.0, "文本宽度应当大于 0");

        let plain = context
            .draw(
                RenderRequest::new(200, 80).with_background(Color::WHITE),
                |_canvas, _context| {},
            )
            .expect("纯色底绘制应当成功");
        let with_text = context
            .draw(
                RenderRequest::new(200, 80).with_background(Color::WHITE),
                |canvas, context| {
                    let drawn = context
                        .draw_text(canvas, "Aurorix", 8.0, 52.0, &style)
                        .expect("文字绘制应当成功");
                    assert!(drawn > 0.0, "绘制返回的字符宽度应当大于 0");
                },
            )
            .expect("文字绘制应当成功");
        assert!(
            with_text.len() > plain.len(),
            "带文字的 PNG 字节数应当大于纯色底"
        );
    })
    .await
    .expect("文字渲染测试超时");
}

#[tokio::test]
async fn larger_font_size_measures_wider() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let small = TextStyle::new("sans-serif", 16);
        let large = TextStyle::new("sans-serif", 48);
        let small_width = context
            .measure_text("Aurorix", &small)
            .expect("小字号测量应当成功");
        let large_width = context
            .measure_text("Aurorix", &large)
            .expect("大字号测量应当成功");
        assert!(
            large_width > small_width,
            "字号变大后测量宽度应当变大: {large_width} <= {small_width}"
        );
    })
    .await
    .expect("字号测量测试超时");
}

#[tokio::test]
async fn invalid_request_is_rejected() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let outcome = context.draw(RenderRequest::new(0, 40), |_canvas, _context| {});
        match outcome {
            Err(RenderError::Backend(message)) => {
                assert!(
                    message.contains("画布尺寸非法"),
                    "零宽画布应当被尺寸守卫拒绝"
                );
            }
            other => panic!("零宽画布应当被拒绝, 实际为 {other:?}"),
        }
        let outcome = context.draw(RenderRequest::new(40, -8), |_canvas, _context| {});
        match outcome {
            Err(RenderError::Backend(message)) => {
                assert!(
                    message.contains("画布尺寸非法"),
                    "负高画布应当被尺寸守卫拒绝"
                );
            }
            other => panic!("负高画布应当被拒绝, 实际为 {other:?}"),
        }
    })
    .await
    .expect("非法请求测试超时");
}

#[tokio::test]
async fn text_is_antialiased() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let style = TextStyle::new("sans-serif", 40);
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((120, 60)).expect("画布应当可分配");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::WHITE);
        context
            .draw_text(canvas, "Aurorix", 4.0, 44.0, &style)
            .expect("文字绘制应当成功");
        let pixmap = surface.peek_pixels().expect("光栅画布应当可以读取像素");
        let mut soft = 0usize;
        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                let red = pixmap.get_color((x, y)).r();
                if red > 0 && red < 255 {
                    soft += 1;
                }
            }
        }
        assert!(soft > 0, "文字边缘应当出现抗锯齿过渡像素");
    })
    .await
    .expect("抗锯齿测试超时");
}

#[test]
fn semibold_weight_counts_as_bold() {
    use aurorix_render::FontWeight;
    assert!(FontWeight::SemiBold.is_bold(), "600 应当算作粗体");
    assert!(FontWeight::Bold.is_bold(), "700 应当算作粗体");
    assert!(FontWeight::Black.is_bold(), "900 应当算作粗体");
    assert!(!FontWeight::Medium.is_bold(), "500 不应算作粗体");
    assert!(!FontWeight::Normal.is_bold(), "400 不应算作粗体");
}

#[tokio::test]
async fn zero_sized_draw_is_rejected() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let error = context
            .draw(RenderRequest::new(0, 32), |_canvas, _context| {})
            .expect_err("宽度为 0 的画布必须被拒绝");
        let message = error.to_string();
        assert!(
            matches!(error, RenderError::Backend(_)),
            "应当返回后端错误, 实际为 {error:?}"
        );
        assert!(
            message.contains("画布尺寸非法"),
            "零宽度必须先被尺寸校验拦下, 实际错误为 {message}"
        );
        let error = context
            .draw(RenderRequest::new(32, 0), |_canvas, _context| {})
            .expect_err("高度为 0 的画布必须被拒绝");
        assert!(
            matches!(error, RenderError::Backend(_)),
            "应当返回后端错误, 实际为 {error:?}"
        );
    })
    .await
    .expect("零尺寸画布测试超时");
}

fn image_pixels(image: &Image) -> (usize, usize, usize, Vec<u8>) {
    let mut surface = skia_safe::surfaces::raster_n32_premul((image.width(), image.height()))
        .expect("离屏画布应当可分配");
    surface.canvas().draw_image(image, (0, 0), None);
    let pixmap = surface.peek_pixels().expect("光栅画布应当可以读取像素");
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    let row = pixmap.row_bytes();
    let pixels = pixmap.bytes().expect("像素缓冲应当可以直接读取").to_vec();
    (width, height, row, pixels)
}

#[tokio::test]
async fn hidpi_scale_allocates_physical_surface() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let request = RenderRequest::new(100, 50).with_scale(2.0);
        assert_eq!(
            request.physical_size(),
            (200, 100),
            "scale 2.0 应当把 100x50 放大到 200x100"
        );
        assert_eq!(
            request.estimated_bytes(),
            200 * 100 * 4,
            "预估字节数应当按物理像素计算"
        );
        let bytes = context
            .draw(request, |canvas, _context| {
                fill_rect(canvas, 0.0, 0.0, 10.0, 10.0, Color::BLACK);
            })
            .expect("高清渲染应当成功");
        let image = decode(&bytes);
        assert_eq!(image.width(), 200, "编码输出的宽度应当是物理宽度");
        assert_eq!(image.height(), 100, "编码输出的高度应当是物理高度");
    })
    .await
    .expect("高清缩放测试超时");
}

#[tokio::test]
async fn hidpi_scale_maps_logical_rect_to_physical_pixels() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let request = RenderRequest::new(100, 50)
            .with_scale(2.0)
            .with_format(RenderFormat::Png);
        let bytes = context
            .draw(request, |canvas, _context| {
                fill_rect(canvas, 0.0, 0.0, 10.0, 10.0, Color::BLACK);
            })
            .expect("高清渲染应当成功");
        let image = decode(&bytes);
        let (_, _, row, pixels) = image_pixels(&image);
        let alpha_at = |x: usize, y: usize| pixels[y * row + x * 4 + 3];
        assert_eq!(alpha_at(19, 19), 255, "逻辑 10x10 应当覆盖到物理 20x20");
        assert_eq!(alpha_at(19, 0), 255, "物理上边缘应当被覆盖");
        assert_eq!(alpha_at(0, 19), 255, "物理左边缘应当被覆盖");
        assert_eq!(alpha_at(24, 24), 0, "物理 20x20 之外应当保持透明");
        let opaque = (0..50)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .filter(|(x, y)| alpha_at(*x, *y) > 128)
            .count();
        assert!(
            (380..=420).contains(&opaque),
            "物理覆盖面积应当接近 400 像素, 实际为 {opaque}"
        );
    })
    .await
    .expect("高清像素映射测试超时");
}

#[test]
fn scale_is_clamped_to_sane_range() {
    assert_eq!(
        RenderRequest::new(10, 10).with_scale(9.0).scale,
        4.0,
        "倍率上限应当被夹到 4.0"
    );
    assert_eq!(
        RenderRequest::new(10, 10).with_scale(0.25).scale,
        1.0,
        "倍率下限应当被夹到 1.0"
    );
    assert_eq!(
        RenderRequest::new(10, 10).with_scale(f32::NAN).scale,
        1.0,
        "非法倍率应当回落到 1.0"
    );
    assert_eq!(
        RenderRequest::new(10, 10).with_scale(1.5).physical_size(),
        (15, 15),
        "1.5 倍应当得到 15x15 物理画布"
    );
    assert_eq!(
        RenderRequest::new(11, 11).with_scale(1.5).physical_size(),
        (17, 17),
        "物理尺寸应当向上取整"
    );
}
