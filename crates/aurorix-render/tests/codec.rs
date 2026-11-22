use aurorix_render::codec::{encode_rgba, EncodeOptions};
use aurorix_render::{
    default_webp_method, encode_surface, webp, Color, RenderContext, RenderError, RenderFormat,
    RenderRequest, SkiaBackend,
};
use skia_safe::{surfaces, AlphaType, ColorType, ImageInfo};
use std::sync::Arc;

const WIDTH: i32 = 96;
const HEIGHT: i32 = 64;

fn payload(quality: u8) -> Vec<u8> {
    let mut buffer = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let r = ((x * 2) % 256) as u8;
            let g = ((y * 4) % 256) as u8;
            let b = (((x + y) * 3) % 256) as u8;
            let a = if (x + y + i32::from(quality)) % 3 == 0 {
                128
            } else {
                255
            };
            buffer.extend_from_slice(&[r, g, b, a]);
        }
    }
    buffer
}

fn encode(format: RenderFormat, options: EncodeOptions) -> Vec<u8> {
    let rgba = payload(options.quality);
    let bytes = encode_rgba(format, &rgba, WIDTH, HEIGHT, WIDTH as usize * 4, &options)
        .expect("编码应当成功");
    bytes.to_vec()
}

fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.starts_with(b"\x89PNG"), "PNG 魔数应当正确");
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (width, height)
}

fn webp_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(&bytes[0..4], b"RIFF", "webp 容器头应当为 RIFF");
    assert_eq!(&bytes[8..12], b"WEBP", "webp 标志应当为 WEBP");
    let decoded = webp::Decoder::new(bytes).decode().expect("webp 应当可解码");
    (decoded.width(), decoded.height())
}

#[test]
fn three_formats_have_correct_magic() {
    let png = encode(RenderFormat::Png, EncodeOptions::new(90));
    assert_eq!(
        png_size(&png),
        (WIDTH as u32, HEIGHT as u32),
        "PNG 尺寸应当一致"
    );

    let jpeg = encode(RenderFormat::Jpeg, EncodeOptions::new(85));
    assert!(
        jpeg.starts_with(&[0xff, 0xd8, 0xff]),
        "JPEG 魔数应当为 FFD8FF"
    );

    let webp_bytes = encode(RenderFormat::WebP, EncodeOptions::new(80));
    assert_eq!(
        webp_size(&webp_bytes),
        (WIDTH as u32, HEIGHT as u32),
        "webp 尺寸应当一致"
    );
}

#[test]
fn lossless_webp_round_trips_pixels_exactly() {
    let rgba = payload(70);
    let options = EncodeOptions::new(80).with_lossless(true);
    let bytes = encode_rgba(
        RenderFormat::WebP,
        &rgba,
        WIDTH,
        HEIGHT,
        WIDTH as usize * 4,
        &options,
    )
    .expect("无损 webp 编码应当成功");
    let decoded = webp::Decoder::new(&bytes)
        .decode()
        .expect("无损 webp 应当可解码");
    assert_eq!(
        (decoded.width(), decoded.height()),
        (WIDTH as u32, HEIGHT as u32),
        "无损 webp 应当保持尺寸"
    );
    assert_eq!(decoded.len(), rgba.len(), "无损 webp 解码后字节数应当一致");
    let pixels = decoded.to_vec();
    let mut mismatched = 0usize;
    for (left, right) in pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(rgba.as_chunks::<4>().0)
    {
        if left[3] == 255 && right[3] == 255 && left[..3] != right[..3] {
            mismatched += 1;
        }
    }
    assert_eq!(mismatched, 0, "无损 webp 应当逐像素还原 RGB 通道顺序");
}

#[test]
fn quality_is_monotonic_for_lossy_formats() {
    for format in [RenderFormat::Jpeg, RenderFormat::WebP] {
        let low = encode(format, EncodeOptions::new(1)).len();
        let high = encode(format, EncodeOptions::new(100)).len();
        assert!(
            low < high,
            "{} 质量 1 的字节数应当小于质量 100: {low} vs {high}",
            format.name()
        );
    }
}

#[test]
fn invalid_quality_is_rejected_in_chinese() {
    let rgba = payload(50);
    for quality in [0u8, 101u8] {
        let outcome = encode_rgba(
            RenderFormat::WebP,
            &rgba,
            WIDTH,
            HEIGHT,
            WIDTH as usize * 4,
            &EncodeOptions::new(quality),
        );
        match outcome {
            Err(RenderError::Encode(message)) => {
                assert!(
                    message.contains("编码质量"),
                    "非法质量错误应当包含中文说明, 实际为 {message}"
                );
            }
            other => panic!("非法质量 {quality} 应当被拒绝, 实际为 {other:?}"),
        }
    }
    let outcome = encode_rgba(
        RenderFormat::Png,
        &rgba,
        WIDTH,
        HEIGHT,
        WIDTH as usize * 4,
        &EncodeOptions::new(0),
    );
    assert!(
        matches!(outcome, Err(RenderError::Encode(_))),
        "PNG 也应当校验质量区间"
    );
}

#[test]
fn padded_rows_are_packed_before_encoding() {
    let stride = WIDTH as usize * 4 + 12;
    let mut buffer = vec![0u8; stride * HEIGHT as usize];
    let packed = payload(60);
    for row in 0..HEIGHT as usize {
        let source = &packed[row * WIDTH as usize * 4..(row + 1) * WIDTH as usize * 4];
        buffer[row * stride..row * stride + WIDTH as usize * 4].copy_from_slice(source);
    }
    let bytes = encode_rgba(
        RenderFormat::WebP,
        &buffer,
        WIDTH,
        HEIGHT,
        stride,
        &EncodeOptions::new(80),
    )
    .expect("带行填充的缓冲应当可以编码");
    assert_eq!(
        webp_size(&bytes),
        (WIDTH as u32, HEIGHT as u32),
        "带行填充的缓冲编码后尺寸应当一致"
    );

    let png = encode_rgba(
        RenderFormat::Png,
        &buffer,
        WIDTH,
        HEIGHT,
        stride,
        &EncodeOptions::new(80),
    )
    .expect("带行填充的缓冲应当可以编码为 PNG");
    assert_eq!(
        png_size(&png),
        (WIDTH as u32, HEIGHT as u32),
        "PNG 也应当正确处理行填充"
    );
}

#[test]
fn short_buffer_is_rejected_in_chinese() {
    let rgba = payload(50);
    let outcome = encode_rgba(
        RenderFormat::WebP,
        &rgba[..128],
        WIDTH,
        HEIGHT,
        WIDTH as usize * 4,
        &EncodeOptions::new(80),
    );
    match outcome {
        Err(RenderError::Encode(message)) => {
            assert!(
                message.contains("RGBA 缓冲不足"),
                "缓冲不足应当有中文说明, 实际为 {message}"
            );
        }
        other => panic!("缓冲不足应当被拒绝, 实际为 {other:?}"),
    }
}

#[test]
fn surface_encoding_matches_requested_format() {
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 8 * 1024 * 1024);
    for format in [RenderFormat::Png, RenderFormat::Jpeg, RenderFormat::WebP] {
        let request = RenderRequest::new(120, 80)
            .with_format(format)
            .with_background(Color::rgb(20, 30, 40))
            .with_quality(82);
        let bytes = context
            .draw_with_options(request, EncodeOptions::new(82), |canvas, _context| {
                aurorix_render::fill_rect(canvas, 8.0, 8.0, 40.0, 24.0, Color::WHITE);
            })
            .expect("三种格式都应当可以编码");
        match format {
            RenderFormat::Png => {
                assert_eq!(png_size(&bytes), (120, 80), "PNG 尺寸应当一致");
            }
            RenderFormat::Jpeg => {
                assert!(bytes.starts_with(&[0xff, 0xd8, 0xff]), "JPEG 魔数应当正确");
            }
            RenderFormat::WebP => {
                assert_eq!(webp_size(&bytes), (120, 80), "webp 尺寸应当一致");
            }
        }
    }
}

#[test]
fn surface_snapshot_path_reuses_zero_copy_pixels() {
    let info = ImageInfo::new((64, 48), ColorType::RGBA8888, AlphaType::Premul, None);
    let mut buffer = vec![0u8; info.compute_byte_size(info.min_row_bytes())];
    let mut surface = surfaces::wrap_pixels(&info, &mut buffer, info.min_row_bytes(), None)
        .expect("像素缓冲应当可以包装为画布");
    surface
        .canvas()
        .clear(skia_safe::Color::from_argb(255, 40, 80, 120));
    let bytes = encode_surface(&mut surface, RenderFormat::WebP, &EncodeOptions::new(80))
        .expect("零拷贝像素应当可以编码");
    assert_eq!(webp_size(&bytes), (64, 48), "零拷贝编码尺寸应当一致");
}

#[test]
fn default_webp_method_follows_quality_tier() {
    assert_eq!(default_webp_method(40), 0, "低质量档位应当使用最快 method");
    assert_eq!(default_webp_method(60), 1, "中等质量档位应当提速");
    assert_eq!(default_webp_method(80), 2, "默认质量档位应当使用 method 2");
    assert_eq!(default_webp_method(95), 4, "高质量档位应当换取更小体积");
    let options = EncodeOptions::new(80).with_webp_method(6);
    assert_eq!(
        options.effective_webp_method(),
        6,
        "显式 method 应当优先生效"
    );
    let invalid = EncodeOptions::new(80).with_webp_method(9);
    assert!(invalid.validate().is_err(), "超出范围的 method 应当报错");
}

#[test]
fn padded_rows_encode_identically_to_packed_rows() {
    let width = 9i32;
    let height = 5usize;
    let packed_row = width as usize * 4;
    let padded_row = packed_row + 12;
    let mut padded = vec![0u8; padded_row * height];
    for y in 0..height {
        for x in 0..width as usize {
            let offset = y * padded_row + x * 4;
            padded[offset] = (x * 17 + y * 3) as u8;
            padded[offset + 1] = (x * 5 + y * 29) as u8;
            padded[offset + 2] = (x * 11 + y * 7) as u8;
            padded[offset + 3] = 0xFF;
        }
        for x in packed_row..padded_row {
            padded[y * padded_row + x] = 0x7F;
        }
    }
    let packed: Vec<u8> = (0..height)
        .flat_map(|y| {
            let start = y * padded_row;
            padded[start..start + packed_row].to_vec()
        })
        .collect();
    let options = EncodeOptions::default();
    for format in [RenderFormat::Png, RenderFormat::Jpeg, RenderFormat::WebP] {
        let from_padded = encode_rgba(format, &padded, width, height as i32, padded_row, &options)
            .expect("带行填充的缓冲应当可以编码");
        let from_packed = encode_rgba(format, &packed, width, height as i32, packed_row, &options)
            .expect("紧凑缓冲应当可以编码");
        assert_eq!(
            from_padded,
            from_packed,
            "{} 编码必须逐行忽略行填充字节",
            format.name()
        );
    }
}

#[test]
fn zero_width_is_rejected() {
    let options = EncodeOptions::default();
    let error = encode_rgba(RenderFormat::Png, &[], 0, 4, 0, &options)
        .expect_err("宽度为 0 时必须拒绝编码");
    let message = error.to_string();
    assert!(
        matches!(error, RenderError::Encode(_)),
        "应当返回编码错误, 实际为 {error:?}"
    );
    assert!(
        message.contains("尺寸非法"),
        "零宽度必须先被尺寸校验拦下, 实际错误为 {message}"
    );
    let tall = vec![0u8; 16];
    let error = encode_rgba(RenderFormat::Png, &tall, 4, 0, 16, &options)
        .expect_err("高度为 0 时必须拒绝编码");
    assert!(
        matches!(error, RenderError::Encode(_)),
        "应当返回编码错误, 实际为 {error:?}"
    );
}

#[test]
fn quality_presets_pick_format_and_quality() {
    let crisp = aurorix_render::RenderQuality::CrispPng.apply(RenderRequest::new(10, 10));
    assert_eq!(crisp.format, RenderFormat::Png, "CrispPng 应当选择 PNG");
    assert_eq!(crisp.quality, 100, "CrispPng 应当使用最高质量");

    let webp = aurorix_render::RenderQuality::HighWebp.apply(RenderRequest::new(10, 10));
    assert_eq!(webp.format, RenderFormat::WebP, "HighWebp 应当选择 WebP");
    assert_eq!(webp.quality, 92, "HighWebp 应当使用 92 质量");

    let jpeg = aurorix_render::RenderQuality::HighJpeg.apply(RenderRequest::new(10, 10));
    assert_eq!(jpeg.format, RenderFormat::Jpeg, "HighJpeg 应当选择 JPEG");
    assert_eq!(jpeg.quality, 92, "HighJpeg 应当使用 92 质量");
}

#[test]
fn quality_presets_map_to_encode_options() {
    let crisp = aurorix_render::RenderQuality::CrispPng.encode_options();
    assert_eq!(crisp.quality, 100, "CrispPng 编码质量应当为 100");
    assert!(!crisp.lossless, "PNG 默认不切换到无损 WebP 模式");
    assert_eq!(crisp.webp_method, None, "PNG 不应设置 webp method");
    assert!(!crisp.jpeg_progressive, "PNG 不应启用 JPEG 渐进式");

    let webp = aurorix_render::RenderQuality::HighWebp.encode_options();
    assert_eq!(webp.quality, 92, "HighWebp 编码质量应当为 92");
    assert_eq!(
        webp.webp_method,
        Some(6),
        "HighWebp 应当使用最高压缩方法 (banded)"
    );
    assert_eq!(webp.effective_webp_method(), 6, "生效方法应当是 6");

    let jpeg = aurorix_render::RenderQuality::HighJpeg.encode_options();
    assert_eq!(jpeg.quality, 92, "HighJpeg 编码质量应当为 92");
    assert!(jpeg.jpeg_progressive, "HighJpeg 应当启用渐进式 JPEG");
    jpeg.validate().expect("预设的编码参数必须合法");
    webp.validate().expect("预设的编码参数必须合法");
    crisp.validate().expect("预设的编码参数必须合法");
}

#[test]
fn quality_presets_produce_decodable_output() {
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 8 * 1024 * 1024);
    for preset in [
        aurorix_render::RenderQuality::CrispPng,
        aurorix_render::RenderQuality::HighWebp,
        aurorix_render::RenderQuality::HighJpeg,
    ] {
        let request = preset
            .apply(RenderRequest::new(48, 32))
            .with_background(Color::rgb(20, 40, 60));
        let bytes = context
            .draw_with_options(request, preset.encode_options(), |canvas, _context| {
                aurorix_render::fill_rect(canvas, 4.0, 4.0, 20.0, 12.0, Color::WHITE);
            })
            .unwrap_or_else(|error| panic!("{} 预设应当可以渲染: {error}", preset.name()));
        assert!(!bytes.is_empty(), "{} 预设不应产出空字节", preset.name());
    }
}
