use aurorix_log::module;
use bytes::Bytes;
use skia_safe::{AlphaType, ColorType, EncodedImageFormat, ImageInfo, Pixmap, Surface};

use crate::draw::{RenderFormat, RenderRequest};
use crate::error::RenderError;

const MODULE: &str = "render";

pub const DEFAULT_QUALITY: u8 = 85;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeOptions {
    pub quality: u8,
    pub lossless: bool,
    pub webp_method: Option<u8>,
    pub jpeg_progressive: bool,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            quality: DEFAULT_QUALITY,
            lossless: false,
            webp_method: None,
            jpeg_progressive: false,
        }
    }
}

impl EncodeOptions {
    pub fn new(quality: u8) -> Self {
        Self {
            quality,
            ..Self::default()
        }
    }

    pub fn with_quality(mut self, quality: u8) -> Self {
        self.quality = quality;
        self
    }

    pub fn with_lossless(mut self, lossless: bool) -> Self {
        self.lossless = lossless;
        self
    }

    pub fn with_webp_method(mut self, method: u8) -> Self {
        self.webp_method = Some(method);
        self
    }

    pub fn with_jpeg_progressive(mut self, progressive: bool) -> Self {
        self.jpeg_progressive = progressive;
        self
    }


    pub fn from_request(request: &RenderRequest) -> Self {
        Self {
            quality: request.quality.clamp(1, 100),
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), RenderError> {
        if !(1..=100).contains(&self.quality) {
            return Err(RenderError::encode(format!(
                "编码质量必须在 1..=100 之间, 实际为 {}",
                self.quality
            )));
        }
        if let Some(method) = self.webp_method {
            if method > 6 {
                return Err(RenderError::encode(format!(
                    "webp 编码 method 必须在 0..=6 之间, 实际为 {method}"
                )));
            }
        }
        Ok(())
    }

    pub fn effective_webp_method(&self) -> u8 {
        match self.webp_method {
            Some(method) => method.min(6),
            None => default_webp_method(self.quality),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedImage {
    pub bytes: Bytes,
    pub format: RenderFormat,
}

impl RenderedImage {
    pub fn mime(&self) -> &'static str {
        self.format.mime()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderQuality {
    #[default]
    CrispPng,
    HighWebp,
    HighJpeg,
}

impl RenderQuality {
    pub const MAX_QUALITY: u8 = 100;
    pub const HIGH_QUALITY: u8 = 92;
    pub const WEBP_METHOD_BEST: u8 = 6;

    pub fn name(self) -> &'static str {
        match self {
            Self::CrispPng => "crisp-png",
            Self::HighWebp => "high-webp",
            Self::HighJpeg => "high-jpeg",
        }
    }

    pub fn apply(self, request: RenderRequest) -> RenderRequest {
        match self {
            Self::CrispPng => request
                .with_format(RenderFormat::Png)
                .with_quality(Self::MAX_QUALITY),
            Self::HighWebp => request
                .with_format(RenderFormat::WebP)
                .with_quality(Self::HIGH_QUALITY),
            Self::HighJpeg => request
                .with_format(RenderFormat::Jpeg)
                .with_quality(Self::HIGH_QUALITY),
        }
    }

    pub fn encode_options(self) -> EncodeOptions {
        match self {
            Self::CrispPng => EncodeOptions::new(Self::MAX_QUALITY),
            Self::HighWebp => {
                EncodeOptions::new(Self::HIGH_QUALITY).with_webp_method(Self::WEBP_METHOD_BEST)
            }
            Self::HighJpeg => EncodeOptions::new(Self::HIGH_QUALITY).with_jpeg_progressive(true),
        }
    }
}

pub fn default_webp_method(quality: u8) -> u8 {
    match quality {
        0..=49 => 0,
        50..=74 => 1,
        75..=89 => 2,
        _ => 4,
    }
}

pub fn encode_surface(
    surface: &mut Surface,
    format: RenderFormat,
    options: &EncodeOptions,
) -> Result<Bytes, RenderError> {
    options.validate()?;
    match format {
        RenderFormat::Png => {
            let image = surface.image_snapshot();
            let quality = u32::from(options.quality.clamp(1, 100));
            let encoded = image
                .encode(None, format.to_skia(), quality)
                .ok_or_else(|| {
                    RenderError::Backend(format!(
                        "{} 格式编码失败 ({}x{})",
                        format.name(),
                        image.width(),
                        image.height()
                    ))
                })?;
            Ok(Bytes::copy_from_slice(encoded.as_bytes()))
        }
        RenderFormat::Jpeg => {
            let (width, height) = (surface.width(), surface.height());
            let pixmap = surface
                .canvas()
                .peek_pixels()
                .ok_or_else(|| RenderError::Backend(String::from("光栅画布无法读取像素")))?;
            let row_bytes = pixmap.row_bytes();
            let pixels = pixmap
                .bytes()
                .ok_or_else(|| RenderError::Backend(String::from("光栅像素缓冲不可直接读取")))?;
            encode_rgba(format, pixels, width, height, row_bytes, options)
        }
        RenderFormat::WebP => {
            let (width, height) = (surface.width(), surface.height());
            let pixmap = surface
                .canvas()
                .peek_pixels()
                .ok_or_else(|| RenderError::Backend(String::from("光栅画布无法读取像素")))?;
            let row_bytes = pixmap.row_bytes();
            let pixels = pixmap
                .bytes()
                .ok_or_else(|| RenderError::Backend(String::from("光栅像素缓冲不可直接读取")))?;
            encode_rgba(format, pixels, width, height, row_bytes, options)
        }
    }
}

pub fn encode_rgba(
    format: RenderFormat,
    rgba: &[u8],
    width: i32,
    height: i32,
    row_bytes: usize,
    options: &EncodeOptions,
) -> Result<Bytes, RenderError> {
    let mut buffer = rgba.to_vec();
    encode_rgba_mut(format, &mut buffer, width, height, row_bytes, options)
}

pub fn encode_rgba_mut(
    format: RenderFormat,
    rgba: &mut [u8],
    width: i32,
    height: i32,
    row_bytes: usize,
    options: &EncodeOptions,
) -> Result<Bytes, RenderError> {
    options.validate()?;
    if width <= 0 || height <= 0 {
        return Err(RenderError::encode(format!(
            "编码尺寸非法: {width}x{height}"
        )));
    }
    let columns = width as usize;
    let height = height as usize;
    let packed_row = columns * 4;
    if row_bytes < packed_row {
        return Err(RenderError::encode(format!(
            "行字节数 {row_bytes} 小于最小宽度 {packed_row}"
        )));
    }
    let required = row_bytes * height;
    if rgba.len() < required {
        return Err(RenderError::encode(format!(
            "RGBA 缓冲不足: 需要 {required} 字节, 实际 {} 字节",
            rgba.len()
        )));
    }
    if row_bytes > packed_row {
        let mut packed = pack_rows(&rgba[..required], packed_row, row_bytes, height);
        return encode_rgba_mut(format, &mut packed, width, height as i32, packed_row, options);
    }
    let buffer = &mut rgba[..required];
    match format {
        RenderFormat::WebP => encode_webp(buffer, columns as u32, height as u32, options),
        RenderFormat::Png => Ok(encode_png(buffer, columns, height)),
        RenderFormat::Jpeg => encode_jpeg(buffer, width, height as i32, packed_row, options),
    }
}

fn encode_with_fallback<F>(
    format: RenderFormat,
    mut encode: F,
) -> Result<RenderedImage, RenderError>
where
    F: FnMut(RenderFormat) -> Result<Bytes, RenderError>,
{
    match encode(format) {
        Ok(bytes) => Ok(RenderedImage { bytes, format }),
        Err(error) if format == RenderFormat::WebP => {
            let fallback = RenderFormat::Jpeg;
            module(MODULE).warn(&format!("webp 编码失败, 出图退回 jpeg: {error}"));
            let bytes = encode(fallback)?;
            Ok(RenderedImage {
                bytes,
                format: fallback,
            })
        }
        Err(error) => Err(error),
    }
}

pub fn encode_rgba_image(
    format: RenderFormat,
    rgba: &[u8],
    width: i32,
    height: i32,
    row_bytes: usize,
    options: &EncodeOptions,
) -> Result<RenderedImage, RenderError> {
    let mut buffer = rgba.to_vec();
    encode_rgba_image_mut(format, &mut buffer, width, height, row_bytes, options)
}

pub fn encode_rgba_image_mut(
    format: RenderFormat,
    rgba: &mut [u8],
    width: i32,
    height: i32,
    row_bytes: usize,
    options: &EncodeOptions,
) -> Result<RenderedImage, RenderError> {
    encode_with_fallback(format, |target| {
        encode_rgba_mut(target, rgba, width, height, row_bytes, options)
    })
}

fn encode_jpeg(
    rgba: &mut [u8],
    width: i32,
    height: i32,
    row_bytes: usize,
    options: &EncodeOptions,
) -> Result<Bytes, RenderError> {
    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let pixmap = Pixmap::new(&info, rgba, row_bytes).ok_or_else(|| {
        RenderError::encode(format!("无法用 {width}x{height} 的 RGBA 缓冲构造位图"))
    })?;
    let jpeg = skia_safe::jpeg_encoder::Options {
        quality: u32::from(options.quality.clamp(1, 100)),
        downsample: skia_safe::jpeg_encoder::Downsample::BothDirections,
        ..Default::default()
    };
    let data = skia_safe::jpeg_encoder::encode_pixmap(&pixmap, &jpeg).ok_or_else(|| {
        RenderError::encode(format!("JPEG 编码失败 ({width}x{height})"))
    })?;
    Ok(Bytes::copy_from_slice(data.as_bytes()))
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn encode_png(rgba: &[u8], width: usize, height: usize) -> Bytes {
    let opaque = rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255);
    let channels = if opaque { 3 } else { 4 };
    let stride = width * channels;
    let mut raw = vec![0u8; (stride + 1) * height];
    if opaque {
        for row in 0..height {
            let source = &rgba[row * width * 4..(row + 1) * width * 4];
            let target = &mut raw[row * (stride + 1)..(row + 1) * (stride + 1)];
            target[0] = 1;
            for (chunk, pixel) in target[1..]
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .zip(source.as_chunks::<4>().0)
            {
                chunk.copy_from_slice(&pixel[..3]);
            }
            for index in (3..stride).rev() {
                target[index + 1] = target[index + 1].wrapping_sub(target[index + 1 - 3]);
            }
        }
    } else {
        for row in 0..height {
            let source = &rgba[row * width * 4..(row + 1) * width * 4];
            let target = &mut raw[row * (stride + 1) + 1..(row + 1) * (stride + 1)];
            for (chunk, pixel) in target
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(source.as_chunks::<4>().0)
            {
                let alpha = pixel[3];
                let straight = match alpha {
                    255 => [pixel[0], pixel[1], pixel[2]],
                    0 => [0, 0, 0],
                    _ => {
                        let scale = |value: u8| {
                            let scaled = (u32::from(value) * 255 + u32::from(alpha) / 2)
                                / u32::from(alpha);
                            scaled.min(255) as u8
                        };
                        [scale(pixel[0]), scale(pixel[1]), scale(pixel[2])]
                    }
                };
                chunk[..3].copy_from_slice(&straight);
                chunk[3] = alpha;
            }
        }
    }
    let idat = fdeflate::compress_to_vec(&raw);
    let mut out = Vec::with_capacity(idat.len() + 64);
    out.extend_from_slice(&PNG_SIGNATURE);
    let mut header = [0u8; 13];
    header[..4].copy_from_slice(&(width as u32).to_be_bytes());
    header[4..8].copy_from_slice(&(height as u32).to_be_bytes());
    header[8] = 8;
    header[9] = if opaque { 2 } else { 6 };
    push_chunk(&mut out, b"IHDR", &header);
    push_chunk(&mut out, b"IDAT", &idat);
    push_chunk(&mut out, b"IEND", &[]);
    Bytes::from(out)
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(kind);
    hasher.update(body);
    out.extend_from_slice(&hasher.finalize().to_be_bytes());
}

fn pack_rows(rgba: &[u8], packed_row: usize, row_bytes: usize, height: usize) -> Vec<u8> {
    let mut packed = vec![0u8; packed_row * height];
    for row in 0..height {
        let source = row * row_bytes;
        let target = row * packed_row;
        packed[target..target + packed_row].copy_from_slice(&rgba[source..source + packed_row]);
    }
    packed
}

fn encode_webp(
    rgba: &[u8],
    width: u32,
    height: u32,
    options: &EncodeOptions,
) -> Result<Bytes, RenderError> {
    let mut config = webp::WebPConfig::new()
        .map_err(|_| RenderError::encode(String::from("webp 编码器初始化失败")))?;
    config.quality = f32::from(options.quality.clamp(1, 100));
    config.lossless = if options.lossless { 1 } else { 0 };
    config.method = i32::from(options.effective_webp_method());
    config.thread_level = 1;
    config.alpha_quality = i32::from(options.quality.clamp(1, 100));
    config.exact = if options.lossless { 1 } else { 0 };
    config.alpha_compression = if options.lossless { 0 } else { 1 };
    let encoder = webp::Encoder::from_rgba(rgba, width, height);
    let memory = encoder
        .encode_advanced(&config)
        .map_err(|error| RenderError::encode(format!("webp 编码失败: {error:?}")))?;
    Ok(Bytes::copy_from_slice(&memory))
}

pub fn skia_format(format: RenderFormat) -> EncodedImageFormat {
    format.to_skia()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RGBA: [u8; 4] = [200, 40, 40, 255];

    #[test]
    fn webp_failure_falls_back_to_a_decodable_jpeg() {
        let pixels = RGBA.repeat(16);
        let options = EncodeOptions::new(80);
        let image = encode_with_fallback(RenderFormat::WebP, |target| match target {
            RenderFormat::WebP => Err(RenderError::encode("模拟 webp 编码失败")),
            other => encode_rgba(other, &pixels, 4, 4, 16, &options),
        })
        .expect("webp 失败之后必须回退成功");
        assert_eq!(image.format, RenderFormat::Jpeg, "回退目标应当是 jpeg");
        assert_eq!(image.mime(), "image/jpeg", "mime 必须跟随回退后的格式");
        assert!(
            image.bytes.starts_with(&[0xff, 0xd8, 0xff]),
            "回退产物必须是 jpeg 字节"
        );
        let decoded =
            skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&image.bytes)).expect(
                "回退产物必须可以解码回位图",
            );
        assert_eq!((decoded.width(), decoded.height()), (4, 4));
    }

    #[test]
    fn webp_success_keeps_the_requested_format() {
        let pixels = RGBA.repeat(16);
        let options = EncodeOptions::new(80);
        let image = encode_rgba_image(RenderFormat::WebP, &pixels, 4, 4, 16, &options)
            .expect("webp 编码应当成功");
        assert_eq!(image.format, RenderFormat::WebP);
        assert_eq!(image.mime(), "image/webp");
        assert!(image.bytes.starts_with(b"RIFF"), "webp 容器头应当正确");
    }

    #[test]
    fn non_webp_failures_are_reported_unchanged() {
        let outcome =
            encode_with_fallback(RenderFormat::Png, |_| Err(RenderError::encode("模拟 png 失败")));
        assert!(
            matches!(outcome, Err(RenderError::Encode(_))),
            "非 webp 格式的失败必须原样上抛"
        );
    }
}
