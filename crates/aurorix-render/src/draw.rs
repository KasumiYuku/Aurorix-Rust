use skia_safe::{font::Edging, Canvas, EncodedImageFormat, Font, FontHinting, Paint};

use crate::context::FontBook;
use crate::error::RenderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Self = Self::new(255, 255, 255, 255);
    pub const BLACK: Self = Self::new(0, 0, 0, 255);
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    pub fn to_skia(self) -> skia_safe::Color {
        skia_safe::Color::from_argb(self.a, self.r, self.g, self.b)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderFormat {
    Png,
    #[default]
    Jpeg,
    WebP,
}

impl RenderFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::WebP => "webp",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
        }
    }

    pub fn is_opaque(self) -> bool {
        matches!(self, Self::Jpeg)
    }

    pub fn to_skia(self) -> EncodedImageFormat {
        match self {
            Self::Png => EncodedImageFormat::PNG,
            Self::Jpeg => EncodedImageFormat::JPEG,
            Self::WebP => EncodedImageFormat::WEBP,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderRequest {
    pub width: i32,
    pub height: i32,
    pub format: RenderFormat,
    pub quality: u8,
    pub background: Option<Color>,
    pub scale: f32,
}

impl RenderRequest {
    pub const DEFAULT_QUALITY: u8 = 85;
    pub const MIN_SCALE: f32 = 1.0;
    pub const MAX_SCALE: f32 = 4.0;

    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            format: RenderFormat::default(),
            quality: Self::DEFAULT_QUALITY,
            background: None,
            scale: Self::MIN_SCALE,
        }
    }

    pub fn with_format(mut self, format: RenderFormat) -> Self {
        self.format = format;
        self
    }

    pub fn with_quality(mut self, quality: u8) -> Self {
        self.quality = quality;
        self
    }

    pub fn with_background(mut self, background: Color) -> Self {
        self.background = Some(background);
        self
    }

    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = Self::clamp_scale(scale);
        self
    }

    pub fn clamp_scale(scale: f32) -> f32 {
        if scale.is_finite() {
            scale.clamp(Self::MIN_SCALE, Self::MAX_SCALE)
        } else {
            Self::MIN_SCALE
        }
    }

    pub fn effective_scale(&self) -> f32 {
        Self::clamp_scale(self.scale)
    }

    pub fn physical_size(&self) -> (i32, i32) {
        let scale = f64::from(self.effective_scale());
        let width = scale_dimension(self.width, scale);
        let height = scale_dimension(self.height, scale);
        (width, height)
    }

    pub fn effective_background(&self) -> Option<Color> {
        match self.background {
            Some(color) => Some(color),
            None if self.format.is_opaque() => Some(Color::WHITE),
            None => None,
        }
    }

    pub fn estimated_bytes(&self) -> u64 {
        let (width, height) = self.physical_size();
        let pixels = i64::from(width) * i64::from(height);
        (pixels * 4).max(0) as u64
    }
}

fn scale_dimension(logical: i32, scale: f64) -> i32 {
    let physical = (f64::from(logical.max(0)) * scale).ceil();
    if physical >= f64::from(i32::MAX) {
        i32::MAX
    } else {
        physical as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontWeight {
    Thin,
    ExtraLight,
    Light,
    #[default]
    Normal,
    Medium,
    SemiBold,
    Bold,
    ExtraBold,
    Black,
}

impl FontWeight {
    pub fn value(self) -> u16 {
        match self {
            Self::Thin => 100,
            Self::ExtraLight => 200,
            Self::Light => 300,
            Self::Normal => 400,
            Self::Medium => 500,
            Self::SemiBold => 600,
            Self::Bold => 700,
            Self::ExtraBold => 800,
            Self::Black => 900,
        }
    }

    pub fn is_bold(self) -> bool {
        self.value() >= 600
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub family: String,
    pub fallbacks: Vec<String>,
    pub size: u32,
    pub weight: FontWeight,
    pub color: Color,
    pub letter_spacing: f32,
    pub align: TextAlign,
}

impl TextStyle {
    pub fn new(family: impl Into<String>, size: u32) -> Self {
        Self {
            family: family.into(),
            fallbacks: Vec::new(),
            size: size.max(1),
            weight: FontWeight::default(),
            color: Color::BLACK,
            letter_spacing: 0.0,
            align: TextAlign::default(),
        }
    }

    pub fn with_fallback(mut self, family: impl Into<String>) -> Self {
        let family = family.into();
        if family != self.family && !self.fallbacks.contains(&family) {
            self.fallbacks.push(family);
        }
        self
    }

    pub fn with_fallbacks<I, S>(mut self, families: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for family in families {
            self = self.with_fallback(family);
        }
        self
    }

    pub fn families(&self) -> Vec<String> {
        let mut families = Vec::with_capacity(1 + self.fallbacks.len());
        families.push(self.family.clone());
        families.extend(self.fallbacks.iter().cloned());
        families
    }

    pub fn bold(&self) -> bool {
        self.weight.is_bold()
    }

    pub fn with_bold(mut self, bold: bool) -> Self {
        self.weight = if bold {
            FontWeight::Bold
        } else {
            FontWeight::Normal
        };
        self
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_size(mut self, size: u32) -> Self {
        self.size = size.max(1);
        self
    }

    pub fn with_letter_spacing(mut self, letter_spacing: f32) -> Self {
        self.letter_spacing = letter_spacing;
        self
    }

    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self::new("sans-serif", 16)
    }
}

pub fn crisp_font(mut font: Font) -> Font {
    font.set_subpixel(true);
    font.set_baseline_snap(true);
    font.set_linear_metrics(true);
    font.set_edging(Edging::AntiAlias);
    font.set_hinting(FontHinting::Slight);
    font.set_force_auto_hinting(false);
    font.set_embedded_bitmaps(true);
    font.set_embedded_bitmaps(true);
    font
}

pub fn text_paint(color: Color) -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(color.to_skia());
    paint
}

pub fn font_for(fonts: &FontBook, style: &TextStyle) -> Result<Font, RenderError> {
    let bold = style.bold();
    let size = style.size.max(1) as f32;
    let mut last: Option<RenderError> = None;
    for family in style.families() {
        match fonts.typeface(&family, bold) {
            Ok(typeface) => return Ok(crisp_font(Font::new(typeface.duplicate(), size))),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| RenderError::Font(String::from("字体链为空"))))
}

pub fn measure_text(fonts: &FontBook, text: &str, style: &TextStyle) -> Result<f32, RenderError> {
    let font = font_for(fonts, style)?;
    Ok(font.measure_text(text, None).0)
}

pub fn draw_text(
    canvas: &Canvas,
    fonts: &FontBook,
    text: &str,
    x: f32,
    y: f32,
    style: &TextStyle,
) -> Result<f32, RenderError> {
    let font = font_for(fonts, style)?;
    let paint = text_paint(style.color);
    canvas.draw_str(text, (x, y), &font, &paint);
    Ok(font.measure_text(text, None).0)
}

pub fn fill(canvas: &Canvas, color: Color) {
    canvas.clear(color.to_skia());
}

pub fn fill_rect(canvas: &Canvas, left: f32, top: f32, width: f32, height: f32, color: Color) {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(color.to_skia());
    canvas.draw_rect(skia_safe::Rect::from_xywh(left, top, width, height), &paint);
}
