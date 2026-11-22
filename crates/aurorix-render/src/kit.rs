use std::collections::HashSet;
use std::sync::Arc;

use bytes::Bytes;
use skia_safe::{
    canvas::{SaveLayerRec, SrcRectConstraint},
    image_filters, shaders, surfaces, BlendMode, Canvas, ClipOp, Color4f, CubicResampler, Font,
    Image, ImageInfo, Paint, PathBuilder, Point, RRect, RSXform, Rect, SamplingOptions, Shader,
    TextBlob, TileMode, Vector,
};

use crate::batch::{PlacedText, Sprite, TextBlobCache, TextBlobKey};
use crate::cache::{Texture, TextureCache};
use crate::context::{FontBook, RenderContext};
use crate::draw::{crisp_font, text_paint, Color, FontWeight, TextAlign, TextStyle};
use crate::error::RenderError;

const ELLIPSIS: &str = "…";
const MIN_FONT_SIZE: u32 = 1;

const SPRITE_SCALE_EPSILON: f32 = 0.001;

pub const TEXT_BLOB_MAX_BYTES: usize = 64;

pub const DEFAULT_FALLBACKS: [&str; 6] = [
    "Noto Sans CJK SC",
    "Noto Sans CJK JP",
    "Noto Sans CJK KR",
    "DejaVu Sans",
    "Noto Color Emoji",
    "FontAwesome",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontChain {
    primary: String,
    fallbacks: Vec<String>,
}

impl FontChain {
    pub fn new(primary: impl Into<String>) -> Self {
        Self {
            primary: primary.into(),
            fallbacks: Vec::new(),
        }
    }

    pub fn with_default_fallbacks(primary: impl Into<String>) -> Self {
        let mut chain = Self::new(primary);
        for family in DEFAULT_FALLBACKS {
            chain = chain.with_fallback(family);
        }
        chain
    }

    pub fn with_fallback(mut self, family: impl Into<String>) -> Self {
        let family = family.into();
        if family != self.primary && !self.fallbacks.contains(&family) {
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

    pub fn primary(&self) -> &str {
        &self.primary
    }

    pub fn fallbacks(&self) -> &[String] {
        &self.fallbacks
    }

    pub fn families(&self) -> Vec<String> {
        let mut families = vec![self.primary.clone()];
        families.extend(self.fallbacks.iter().cloned());
        families
    }
}

impl Default for FontChain {
    fn default() -> Self {
        Self::with_default_fallbacks("sans-serif")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

impl GradientStop {
    pub fn new(offset: f32, color: Color) -> Self {
        Self {
            offset: offset.clamp(0.0, 1.0),
            color,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LinearDirection {
    Horizontal,
    Vertical,
    Angle(f32),
    Points(Point, Point),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    stops: Vec<GradientStop>,
    direction: LinearDirection,
}

impl LinearGradient {
    pub fn new<I>(stops: I) -> Self
    where
        I: IntoIterator<Item = GradientStop>,
    {
        Self {
            stops: normalize_stops(stops),
            direction: LinearDirection::Horizontal,
        }
    }

    pub fn horizontal<I>(stops: I) -> Self
    where
        I: IntoIterator<Item = GradientStop>,
    {
        Self::new(stops)
    }

    pub fn vertical<I>(stops: I) -> Self
    where
        I: IntoIterator<Item = GradientStop>,
    {
        Self {
            stops: normalize_stops(stops),
            direction: LinearDirection::Vertical,
        }
    }

    pub fn with_angle(mut self, degrees: f32) -> Self {
        self.direction = LinearDirection::Angle(degrees);
        self
    }

    pub fn with_points(mut self, from: impl Into<Point>, to: impl Into<Point>) -> Self {
        self.direction = LinearDirection::Points(from.into(), to.into());
        self
    }

    pub fn with_stop(mut self, stop: GradientStop) -> Self {
        self.stops.push(GradientStop::new(stop.offset, stop.color));
        self.stops.sort_by(|left, right| {
            left.offset
                .partial_cmp(&right.offset)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self
    }

    pub fn shader(&self, bounds: Rect) -> Result<Shader, RenderError> {
        let (colors, positions) = gradient_arrays(&self.stops);
        if colors.is_empty() {
            return Err(RenderError::Backend(String::from(
                "线性渐变至少需要一个色标",
            )));
        }
        let (from, to) = match self.direction {
            LinearDirection::Horizontal => (
                Point::new(bounds.left(), bounds.center_y()),
                Point::new(bounds.right(), bounds.center_y()),
            ),
            LinearDirection::Vertical => (
                Point::new(bounds.center_x(), bounds.top()),
                Point::new(bounds.center_x(), bounds.bottom()),
            ),
            LinearDirection::Angle(degrees) => {
                let radians = degrees.to_radians();
                let half_x = bounds.width().abs() / 2.0;
                let half_y = bounds.height().abs() / 2.0;
                let extent = half_x.hypot(half_y);
                let center = Point::new(bounds.center_x(), bounds.center_y());
                let delta = Point::new(radians.cos() * extent, radians.sin() * extent);
                (
                    Point::new(center.x - delta.x, center.y - delta.y),
                    Point::new(center.x + delta.x, center.y + delta.y),
                )
            }
            LinearDirection::Points(from, to) => (from, to),
        };
        let spec = gradient_spec(&colors, &positions);
        shaders::linear_gradient((from, to), &spec, None)
            .ok_or_else(|| RenderError::Backend(String::from("线性渐变着色器构造失败")))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadialGradient {
    stops: Vec<GradientStop>,
    center: Option<Point>,
    radius: Option<f32>,
}

impl RadialGradient {
    pub fn new<I>(stops: I) -> Self
    where
        I: IntoIterator<Item = GradientStop>,
    {
        Self {
            stops: normalize_stops(stops),
            center: None,
            radius: None,
        }
    }

    pub fn with_center(mut self, center: impl Into<Point>) -> Self {
        self.center = Some(center.into());
        self
    }

    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius.abs());
        self
    }

    pub fn shader(&self, bounds: Rect) -> Result<Shader, RenderError> {
        let (colors, positions) = gradient_arrays(&self.stops);
        if colors.is_empty() {
            return Err(RenderError::Backend(String::from(
                "径向渐变至少需要一个色标",
            )));
        }
        let center = self
            .center
            .unwrap_or_else(|| Point::new(bounds.center_x(), bounds.center_y()));
        let radius = self
            .radius
            .unwrap_or_else(|| bounds.width().abs().min(bounds.height().abs()) / 2.0);
        let spec = gradient_spec(&colors, &positions);
        shaders::radial_gradient((center, radius.max(0.01)), &spec, None)
            .ok_or_else(|| RenderError::Backend(String::from("径向渐变着色器构造失败")))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Gradient {
    Linear(LinearGradient),
    Radial(RadialGradient),
}

impl Gradient {
    pub fn shader(&self, bounds: Rect) -> Result<Shader, RenderError> {
        match self {
            Self::Linear(linear) => linear.shader(bounds),
            Self::Radial(radial) => radial.shader(bounds),
        }
    }
}

impl From<LinearGradient> for Gradient {
    fn from(gradient: LinearGradient) -> Self {
        Self::Linear(gradient)
    }
}

impl From<RadialGradient> for Gradient {
    fn from(gradient: RadialGradient) -> Self {
        Self::Radial(gradient)
    }
}

impl From<&LinearGradient> for Gradient {
    fn from(gradient: &LinearGradient) -> Self {
        Self::Linear(gradient.clone())
    }
}

impl From<&RadialGradient> for Gradient {
    fn from(gradient: &RadialGradient) -> Self {
        Self::Radial(gradient.clone())
    }
}

fn normalize_stops<I>(stops: I) -> Vec<GradientStop>
where
    I: IntoIterator<Item = GradientStop>,
{
    let mut stops: Vec<GradientStop> = stops
        .into_iter()
        .map(|stop| GradientStop::new(stop.offset, stop.color))
        .collect();
    stops.sort_by(|left, right| {
        left.offset
            .partial_cmp(&right.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    stops
}

fn gradient_arrays(stops: &[GradientStop]) -> (Vec<Color4f>, Vec<f32>) {
    let colors = stops
        .iter()
        .map(|stop| Color4f::from(stop.color.to_skia()))
        .collect();
    let positions = stops.iter().map(|stop| stop.offset).collect();
    (colors, positions)
}

fn gradient_spec<'a>(
    colors: &'a [Color4f],
    positions: &'a [f32],
) -> skia_safe::gradient::Gradient<'a> {
    skia_safe::gradient::Gradient::new(
        skia_safe::gradient::Colors::new(colors, Some(positions), TileMode::Clamp, None),
        skia_safe::gradient::Interpolation::default(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    pub color: Color,
    pub width: f32,
}

impl Border {
    pub fn new(color: Color, width: f32) -> Self {
        Self {
            color,
            width: width.max(0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    pub x: f32,
    pub y: f32,
}

impl Padding {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x: x.max(0.0),
            y: y.max(0.0),
        }
    }

    pub fn all(value: f32) -> Self {
        Self::new(value, value)
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self::new(8.0, 4.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

impl TextMetrics {
    pub fn as_tuple(&self) -> (f32, f32, f32) {
        (self.width, self.height, self.baseline)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Truncate,
    Shrink,
    Both,
}

impl FitMode {
    fn truncates(self) -> bool {
        matches!(self, Self::Truncate | Self::Both)
    }

    fn shrinks(self) -> bool {
        matches!(self, Self::Shrink | Self::Both)
    }
}


pub fn default_sampling() -> SamplingOptions {
    sampling_cubic()
}

pub fn sampling_crisp() -> SamplingOptions {
    sampling_cubic()
}

pub fn sampling_cubic() -> SamplingOptions {
    SamplingOptions::from(CubicResampler::catmull_rom())
}

pub fn sampling_linear() -> SamplingOptions {
    SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None)
}

pub fn sampling_nearest() -> SamplingOptions {
    SamplingOptions::default()
}

pub fn sampling_for(source: (f32, f32), dst: Rect) -> SamplingOptions {
    let (sw, sh) = source;
    let (dw, dh) = (dst.width(), dst.height());
    if sw <= 0.0 || sh <= 0.0 || dw <= 0.0 || dh <= 0.0 {
        return sampling_cubic();
    }
    if dw.round() == sw.round() && dh.round() == sh.round() {
        sampling_nearest()
    } else if dw >= sw * 0.98 && dh >= sh * 0.98 {
        sampling_linear()
    } else {
        sampling_cubic()
    }
}

const REDUCE_RATIO: f32 = 1.6;

fn reduced_sampling(
    scaled: &Image,
    crop: Option<Rect>,
    dst: Rect,
) -> SamplingOptions {
    let (sw, sh) = crop.map_or(
        (scaled.width() as f32, scaled.height() as f32),
        |rect| (rect.width(), rect.height()),
    );
    if sw <= 0.0 || sh <= 0.0 {
        return sampling_cubic();
    }
    if sw.round() == dst.width() && sh.round() == dst.height() {
        sampling_nearest()
    } else if sw <= dst.width() * REDUCE_RATIO && sh <= dst.height() * REDUCE_RATIO {
        sampling_linear()
    } else {
        sampling_cubic()
    }
}
pub fn scaled_cache_len() -> usize {
    scaled_cache()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .len()
}
fn snap_rect(rect: Rect) -> Rect {
    Rect::from_xywh(
        rect.left().round(),
        rect.top().round(),
        rect.width().round().max(1.0),
        rect.height().round().max(1.0),
    )
}

type ScaledKey = (u32, i32, i32, i32, i32, i32, i32);

struct ScaledSlot {
    image: Image,
    crop: Option<Rect>,
    pixels: u64,
}

struct ScaledStore {
    slots: std::collections::HashMap<ScaledKey, ScaledSlot>,
    order: std::collections::VecDeque<ScaledKey>,
    total_pixels: u64,
    max_entries: usize,
    max_pixels: u64,
}

impl ScaledStore {
    fn new() -> Self {
        Self::with_limits(SCALED_CACHE_MAX, SCALED_CACHE_MAX_PIXELS)
    }

    fn with_limits(max_entries: usize, max_pixels: u64) -> Self {
        Self {
            slots: std::collections::HashMap::new(),
            order: std::collections::VecDeque::new(),
            total_pixels: 0,
            max_entries: max_entries.max(1),
            max_pixels,
        }
    }

    fn get(&mut self, key: &ScaledKey) -> Option<(Image, Option<Rect>)> {
        let hit = {
            let slot = self.slots.get(key)?;
            (slot.image.clone(), slot.crop)
        };
        if let Some(position) = self.order.iter().position(|hit| hit == key) {
            self.order.remove(position);
        }
        self.order.push_back(*key);
        Some(hit)
    }

    fn put(&mut self, key: ScaledKey, image: Image, crop: Option<Rect>) {
        let pixels = image.width() as u64 * image.height() as u64;
        if let Some(previous) = self.slots.remove(&key) {
            self.total_pixels = self.total_pixels.saturating_sub(previous.pixels);
            if let Some(position) = self.order.iter().position(|hit| *hit == key) {
                self.order.remove(position);
            }
        }
        self.total_pixels += pixels;
        self.slots.insert(key, ScaledSlot { image, crop, pixels });
        self.order.push_back(key);
        while self.slots.len() > 1
            && (self.slots.len() > self.max_entries || self.total_pixels > self.max_pixels)
        {
            let Some(victim) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.slots.remove(&victim) {
                self.total_pixels = self.total_pixels.saturating_sub(removed.pixels);
            }
        }
    }

    fn len(&self) -> usize {
        self.slots.len()
    }
}

const SCALED_CACHE_MAX: usize = 256;
const SCALED_CACHE_MAX_EDGE: i32 = 4096;
const SCALED_CACHE_MAX_DST: i32 = 512;
const SCALED_CACHE_MAX_PIXELS: u64 = 48 * 1024 * 1024;
const BLUR_DOWNSCALE: f32 = 6.0;

type ScaledCache = std::sync::Mutex<ScaledStore>;

static SCALED_CACHE: std::sync::OnceLock<ScaledCache> = std::sync::OnceLock::new();

fn scaled_cache() -> &'static ScaledCache {
    SCALED_CACHE.get_or_init(|| std::sync::Mutex::new(ScaledStore::new()))
}

fn scaled_key(image: &Image, source: Option<Rect>, dst: Rect) -> ScaledKey {
    let (x, y, w, h) = source.map_or((0, 0, 0, 0), |r| {
        (
            r.left() as i32,
            r.top() as i32,
            r.width() as i32,
            r.height() as i32,
        )
    });
    (
        image.unique_id(),
        dst.width() as i32,
        dst.height() as i32,
        x,
        y,
        w,
        h,
    )
}

fn cached_scaled(key: ScaledKey) -> Option<(Image, Option<Rect>)> {
    let mut store = scaled_cache()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    store.get(&key)
}

fn store_scaled(key: ScaledKey, image: &Image, crop: Option<Rect>) {
    if image.width() > SCALED_CACHE_MAX_EDGE || image.height() > SCALED_CACHE_MAX_EDGE {
        return;
    }
    if key.1 > SCALED_CACHE_MAX_DST || key.2 > SCALED_CACHE_MAX_DST {
        return;
    }
    scaled_cache()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .put(key, image.clone(), crop);
}

fn progressive_image(image: &Image, source: Option<Rect>, dst: Rect) -> (Image, Option<Rect>) {
    let key = scaled_key(image, source, dst);
    if let Some(hit) = cached_scaled(key) {
        return hit;
    }
    let mut current = image.clone();
    let mut crop = source;
    let mut steps = 0;
    while steps < 8 {
        steps += 1;
        let (sw, sh) = (current.width(), current.height());
        if sw <= 0 || sh <= 0 {
            break;
        }
        let (cw, ch) = crop.map_or((sw as f32, sh as f32), |r| (r.width(), r.height()));
        if cw <= dst.width() * REDUCE_RATIO && ch <= dst.height() * REDUCE_RATIO {
            break;
        }
        let nw = ((sw + 1) / 2).max(1);
        let nh = ((sh + 1) / 2).max(1);
        if nw == sw && nh == sh {
            break;
        }
        let info = ImageInfo::new_n32_premul((nw, nh), None);
        let Some(mut surface) = surfaces::raster(&info, None, None) else {
            break;
        };
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        surface.canvas().draw_image_rect_with_sampling_options(
            &current,
            None,
            Rect::from_xywh(0.0, 0.0, nw as f32, nh as f32),
            sampling_linear(),
            &paint,
        );
        let kx = nw as f32 / sw as f32;
        let ky = nh as f32 / sh as f32;
        current = surface.image_snapshot();
        crop = crop.map(|r| {
            Rect::from_xywh(r.left() * kx, r.top() * ky, r.width() * kx, r.height() * ky)
        });
    }
    store_scaled(key, &current, crop);
    (current, crop)
}

pub struct TextLayout {
    fonts: Vec<Font>,
}

impl TextLayout {
    pub fn primary_font(&self) -> &Font {
        &self.fonts[0]
    }

    pub fn primary_covers(&self, text: &str) -> bool {
        let primary = &self.fonts[0];
        text.chars()
            .all(|ch| primary.unichar_to_glyph(ch as i32) != 0)
    }

    fn advance(&self, ch: char, letter_spacing: f32) -> f32 {
        let font = self.pick(ch);
        let mut buffer = [0u8; 4];
        let text: &str = ch.encode_utf8(&mut buffer);
        font.measure_str(text, None).0 + letter_spacing
    }

    fn pick(&self, ch: char) -> &Font {
        let unicode = ch as i32;
        for font in &self.fonts {
            if font.unichar_to_glyph(unicode) != 0 {
                return font;
            }
        }
        &self.fonts[self.fonts.len() - 1]
    }

    fn width(&self, text: &str, letter_spacing: f32) -> f32 {
        let count = text.chars().count();
        if count == 0 {
            return 0.0;
        }
        let tracking = letter_spacing * (count - 1) as f32;
        if self.primary_covers(text) {
            return self.fonts[0].measure_str(text, None).0 + tracking;
        }
        text.chars()
            .map(|ch| self.advance(ch, letter_spacing))
            .sum::<f32>()
            - letter_spacing
    }

    fn metrics(&self, text: &str, letter_spacing: f32) -> TextMetrics {
        let primary = &self.fonts[0];
        let (_, font_metrics) = primary.metrics();
        let height = font_metrics.descent - font_metrics.ascent + font_metrics.leading;
        TextMetrics {
            width: self.width(text, letter_spacing),
            height,
            baseline: -font_metrics.ascent,
        }
    }
}

pub trait ImageSource {
    fn load(&self, context: &RenderContext) -> Result<Image, RenderError>;
}

fn cached_image(context: &RenderContext, bytes: &[u8]) -> Result<Image, RenderError> {
    let cache = context.textures();
    let key = crate::cache::content_key(bytes);
    if let Some(hit) = cache.get(&key) {
        match hit.image() {
            Some(image) => return Ok(image.clone()),
            None => cache.remove(&key),
        }
    }
    let texture = cache.image_for(Bytes::copy_from_slice(bytes))?;
    texture
        .image()
        .cloned()
        .ok_or_else(|| RenderError::Texture(String::from("纹理缓存里没有可用的解码位图")))
}

impl ImageSource for Image {
    fn load(&self, _context: &RenderContext) -> Result<Image, RenderError> {
        Ok(self.clone())
    }
}

impl ImageSource for &Image {
    fn load(&self, _context: &RenderContext) -> Result<Image, RenderError> {
        Ok((*self).clone())
    }
}

impl ImageSource for &[u8] {
    fn load(&self, context: &RenderContext) -> Result<Image, RenderError> {
        cached_image(context, self)
    }
}

impl ImageSource for &Vec<u8> {
    fn load(&self, context: &RenderContext) -> Result<Image, RenderError> {
        cached_image(context, self.as_slice())
    }
}

impl ImageSource for &Bytes {
    fn load(&self, context: &RenderContext) -> Result<Image, RenderError> {
        cached_image(context, self.as_ref())
    }
}

pub struct CanvasKit<'a> {
    canvas: &'a Canvas,
    context: &'a RenderContext,
    fonts: &'a FontBook,
    chain: FontChain,
    sampling: SamplingOptions,
}

impl<'a> CanvasKit<'a> {
    pub fn new(canvas: &'a Canvas, context: &'a RenderContext) -> Self {
        Self::with_chain(canvas, context, FontChain::default())
    }

    pub fn with_chain(canvas: &'a Canvas, context: &'a RenderContext, chain: FontChain) -> Self {
        Self {
            canvas,
            context,
            fonts: context.fonts(),
            chain,
            sampling: default_sampling(),
        }
    }

    pub fn with_sampling(mut self, sampling: SamplingOptions) -> Self {
        self.sampling = sampling;
        self
    }

    pub fn sampling(&self) -> SamplingOptions {
        self.sampling
    }

    pub fn canvas(&self) -> &'a Canvas {
        self.canvas
    }

    pub fn fonts(&self) -> &'a FontBook {
        self.fonts
    }

    pub fn context(&self) -> &'a RenderContext {
        self.context
    }

    pub fn textures(&self) -> &'a TextureCache {
        self.context.textures()
    }

    pub fn asset(&self, key: &str) -> Result<Arc<Texture>, RenderError> {
        self.context.asset(key)
    }

    pub fn chain(&self) -> &FontChain {
        &self.chain
    }

    pub fn draw_image(
        &self,
        source: impl ImageSource,
        rect: Rect,
    ) -> Result<&'a Canvas, RenderError> {
        let image = source.load(self.context)?;
        Ok(self.image_rect(&image, rect))
    }

    pub fn draw_image_fit(
        &self,
        source: impl ImageSource,
        rect: Rect,
    ) -> Result<&'a Canvas, RenderError> {
        let image = source.load(self.context)?;
        let target = contain_destination_rect(&image, rect);
        Ok(self.image_rect(&image, target))
    }

    pub fn draw_round_image(
        &self,
        source: impl ImageSource,
        rect: Rect,
        radius: f32,
    ) -> Result<&'a Canvas, RenderError> {
        let image = source.load(self.context)?;
        let target = contain_destination_rect(&image, rect);
        Ok(self.rounded_image(&image, target, radius))
    }

    pub fn layer<R>(
        &self,
        bounds: Rect,
        alpha: f32,
        blend_mode: BlendMode,
        content: impl FnOnce(&Self) -> R,
    ) -> R {
        let mut paint = Paint::default();
        paint.set_alpha((alpha.clamp(0.0, 1.0) * 255.0) as u8);
        paint.set_blend_mode(blend_mode);
        let record = SaveLayerRec::default().bounds(&bounds).paint(&paint);
        self.canvas.save_layer(&record);
        let output = content(self);
        self.canvas.restore();
        output
    }

    pub fn blur_layer<R>(&self, sigma: f32, content: impl FnOnce(&Self) -> R) -> R {
        let bounds = self.layer_bounds();
        self.blur_layer_in(bounds, sigma, content)
    }

    pub fn blur_layer_in<R>(
        &self,
        bounds: Rect,
        sigma: f32,
        content: impl FnOnce(&Self) -> R,
    ) -> R {
        let sigma = sigma.max(0.0);
        let Some(filter) = image_filters::blur((sigma, sigma), TileMode::Decal, None, None) else {
            return content(self);
        };
        let mut paint = Paint::default();
        paint.set_image_filter(filter);
        let record = SaveLayerRec::default().bounds(&bounds).paint(&paint);
        self.canvas.save_layer(&record);
        let output = content(self);
        self.canvas.restore();
        output
    }

    pub fn drop_shadow<R>(
        &self,
        color: Color,
        dx: f32,
        dy: f32,
        sigma: f32,
        content: impl FnOnce(&Self) -> R,
    ) -> R {
        let bounds = self.layer_bounds();
        self.drop_shadow_in(bounds, color, dx, dy, sigma, content)
    }

    pub fn drop_shadow_in<R>(
        &self,
        bounds: Rect,
        color: Color,
        dx: f32,
        dy: f32,
        sigma: f32,
        content: impl FnOnce(&Self) -> R,
    ) -> R {
        let sigma = sigma.max(0.0);
        let Some(shadow) = image_filters::drop_shadow(
            (dx, dy),
            (sigma, sigma),
            Color4f::from(color.to_skia()),
            None,
            None,
            None,
        ) else {
            return content(self);
        };
        let mut paint = Paint::default();
        paint.set_image_filter(shadow);
        let padded = Rect::from_ltrb(
            bounds.left() - sigma * 3.0 - dx.abs(),
            bounds.top() - sigma * 3.0 - dy.abs(),
            bounds.right() + sigma * 3.0 + dx.abs(),
            bounds.bottom() + sigma * 3.0 + dy.abs(),
        );
        let record = SaveLayerRec::default().bounds(&padded).paint(&paint);
        self.canvas.save_layer(&record);
        let output = content(self);
        self.canvas.restore();
        output
    }

    pub fn layer_bounds(&self) -> Rect {
        if let Some(bounds) = self.canvas.local_clip_bounds() {
            if !bounds.is_empty() {
                return bounds;
            }
        }
        Rect::from_xywh(
            0.0,
            0.0,
            self.canvas.image_info().width() as f32,
            self.canvas.image_info().height() as f32,
        )
    }

    pub fn rrect_fill(&self, rect: Rect, radius: f32, color: Color) -> &'a Canvas {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(color.to_skia());
        self.canvas.draw_rrect(rounded(rect, radius), &paint);
        self.canvas
    }

    pub fn fill_rect(&self, rect: Rect, color: Color) -> &'a Canvas {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(color.to_skia());
        self.canvas.draw_rect(rect, &paint);
        self.canvas
    }

    pub fn fill_gradient(
        &self,
        rect: Rect,
        radius: f32,
        gradient: &Gradient,
    ) -> Result<&'a Canvas, RenderError> {
        let shader = gradient.shader(rect)?;
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_shader(shader);
        self.canvas.draw_rrect(rounded(rect, radius), &paint);
        Ok(self.canvas)
    }

    pub fn stroke(&self, rect: Rect, radius: f32, border: Border) -> &'a Canvas {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_style(skia_safe::paint::Style::Stroke);
        paint.set_stroke_width(border.width.max(0.0));
        paint.set_color(border.color.to_skia());
        self.canvas.draw_rrect(rounded(rect, radius), &paint);
        self.canvas
    }

    pub fn circle(&self, center: impl Into<Point>, radius: f32, color: Color) -> &'a Canvas {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(color.to_skia());
        self.canvas.draw_circle(center.into(), radius, &paint);
        self.canvas
    }

    pub fn pill(
        &self,
        rect: Rect,
        gradient: &Gradient,
        border: Option<Border>,
    ) -> Result<&'a Canvas, RenderError> {
        let radius = rect.height().abs().min(rect.width().abs()) / 2.0;
        self.fill_gradient(rect, radius, gradient)?;
        if let Some(border) = border {
            if border.width > 0.0 {
                let inset = border.width / 2.0;
                let inner = Rect::from_ltrb(
                    rect.left() + inset,
                    rect.top() + inset,
                    rect.right() - inset,
                    rect.bottom() - inset,
                );
                self.stroke(inner, radius, border);
            }
        }
        Ok(self.canvas)
    }

    pub fn rounded_image(&self, image: &Image, rect: Rect, radius: f32) -> &'a Canvas {
        let (w, h) = (image.width() as f32, image.height() as f32);
        self.rounded_image_with_sampling(image, rect, radius, sampling_for((w, h), rect))
    }

    pub fn rounded_image_with_sampling(
        &self,
        image: &Image,
        rect: Rect,
        radius: f32,
        sampling: SamplingOptions,
    ) -> &'a Canvas {
        self.canvas.save();
        self.canvas
            .clip_rrect(rounded(rect, radius), ClipOp::Intersect, true);
        self.draw_image_with_sampling(image, None, rect, sampling);
        self.canvas.restore();
        self.canvas
    }

    pub fn image_rect(&self, image: &Image, rect: Rect) -> &'a Canvas {
        let (w, h) = (image.width() as f32, image.height() as f32);
        self.image_rect_with_sampling(image, rect, sampling_for((w, h), rect))
    }

    pub fn image_rect_with_sampling(
        &self,
        image: &Image,
        rect: Rect,
        sampling: SamplingOptions,
    ) -> &'a Canvas {
        self.draw_image_with_sampling(image, None, rect, sampling)
    }

    pub fn image_rect_src(&self, image: &Image, source: Rect, rect: Rect) -> &'a Canvas {
        self.draw_image_with_sampling(
            image,
            Some((&source, SrcRectConstraint::Fast)),
            rect,
            sampling_for((source.width(), source.height()), rect),
        )
    }

    pub fn cover_image(&self, image: &Image, rect: Rect) -> &'a Canvas {
        let source = cover_source_rect(image, rect);
        self.draw_image_with_sampling(
            image,
            Some((&source, SrcRectConstraint::Fast)),
            rect,
            sampling_for((source.width(), source.height()), rect),
        )
    }

    pub fn cover_image_with_sampling(
        &self,
        image: &Image,
        rect: Rect,
        sampling: SamplingOptions,
    ) -> &'a Canvas {
        let source = cover_source_rect(image, rect);
        self.draw_image_with_sampling(
            image,
            Some((&source, SrcRectConstraint::Fast)),
            rect,
            sampling,
        )
    }

    pub fn cover_blurred(&self, image: &Image, rect: Rect, sigma: f32) -> &'a Canvas {
        let dst = snap_rect(rect);
        let factor = 1.0 / BLUR_DOWNSCALE;
        let small_w = ((dst.width() * factor).round() as i32).max(1);
        let small_h = ((dst.height() * factor).round() as i32).max(1);
        if sigma <= 0.5 || small_w < 4 || small_h < 4 {
            return self.cover_image_with_sampling(image, dst, sampling_linear());
        }
        let info = ImageInfo::new_n32_premul((small_w, small_h), None);
        let Some(mut surface) = surfaces::raster(&info, None, None) else {
            return self.cover_image_with_sampling(image, dst, sampling_linear());
        };
        let small = Rect::from_xywh(0.0, 0.0, small_w as f32, small_h as f32);
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        if let Some(filter) =
            image_filters::blur((sigma * factor, sigma * factor), TileMode::Clamp, None, None)
        {
            paint.set_image_filter(filter);
        }
        let source = cover_source_rect(image, small);
        surface.canvas().draw_image_rect_with_sampling_options(
            image,
            Some((&source, SrcRectConstraint::Fast)),
            small,
            sampling_linear(),
            &paint,
        );
        let snapshot = surface.image_snapshot();
        let mut blit = Paint::default();
        blit.set_anti_alias(true);
        self.canvas.draw_image_rect_with_sampling_options(
            &snapshot,
            None,
            dst,
            sampling_linear(),
            &blit,
        );
        self.canvas
    }

    fn draw_image_with_sampling(
        &self,
        image: &Image,
        source: Option<(&Rect, SrcRectConstraint)>,
        rect: Rect,
        sampling: SamplingOptions,
    ) -> &'a Canvas {
        let dst = snap_rect(rect);
        let (scaled, crop) = progressive_image(image, source.map(|(r, _)| *r), dst);
        let crop = crop.map(|r| (r, SrcRectConstraint::Fast));
        let sampling = match sampling.use_cubic {
            true => reduced_sampling(&scaled, crop.as_ref().map(|(rect, _)| *rect), dst),
            false => sampling,
        };
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        self.canvas.draw_image_rect_with_sampling_options(
            &scaled,
            crop.as_ref().map(|(r, mode)| (r, *mode)),
            dst,
            sampling,
            &paint,
        );
        self.canvas
    }

    pub fn draw_asset(&self, key: &str, rect: Rect, radius: f32) -> Result<(), RenderError> {
        let texture = self.asset(key)?;
        let image = texture.image().ok_or_else(|| {
            RenderError::Texture(format!("素材 {key} 没有解码后的位图, 无法绘制"))
        })?;
        let target = contain_destination_rect(image, rect);
        let (w, h) = (image.width() as f32, image.height() as f32);
        self.rounded_image_with_sampling(image, target, radius, sampling_for((w, h), target));
        Ok(())
    }

    pub fn draw_asset_cover(&self, key: &str, rect: Rect, radius: f32) -> Result<(), RenderError> {
        let texture = self.asset(key)?;
        let image = texture.image().ok_or_else(|| {
            RenderError::Texture(format!("素材 {key} 没有解码后的位图, 无法绘制"))
        })?;
        let source = cover_source_rect(image, rect);
        self.canvas.save();
        self.canvas
            .clip_rrect(rounded(rect, radius), ClipOp::Intersect, true);
        self.draw_image_with_sampling(
            image,
            Some((&source, SrcRectConstraint::Fast)),
            rect,
            sampling_for((source.width(), source.height()), rect),
        );
        self.canvas.restore();
        Ok(())
    }

    pub fn fill_rects_batch(&self, rects: &[Rect], color: Color) -> &'a Canvas {
        if rects.is_empty() {
            return self.canvas;
        }
        let mut path = PathBuilder::new();
        for rect in rects {
            path.add_rect(*rect, None, None);
        }
        self.canvas.draw_path(&path.detach(), &solid_paint(color));
        self.canvas
    }

    pub fn fill_round_rects_batch(&self, items: &[(Rect, f32)], color: Color) -> &'a Canvas {
        if items.is_empty() {
            return self.canvas;
        }
        let mut path = PathBuilder::new();
        for (rect, radius) in items {
            path.add_rrect(rounded(*rect, *radius), None, None);
        }
        self.canvas.draw_path(&path.detach(), &solid_paint(color));
        self.canvas
    }

    pub fn draw_sprites_batch(&self, image: &Image, sprites: &[Sprite]) -> &'a Canvas {
        if sprites.is_empty() {
            return self.canvas;
        }
        let mut xforms: Vec<RSXform> = Vec::with_capacity(sprites.len());
        let mut sources: Vec<Rect> = Vec::with_capacity(sprites.len());
        let mut rest: Vec<Sprite> = Vec::new();
        for sprite in sprites {
            let (scale_x, scale_y) = sprite.scale();
            if (scale_x - scale_y).abs() > SPRITE_SCALE_EPSILON {
                rest.push(*sprite);
                continue;
            }
            xforms.push(RSXform::new(
                scale_x,
                0.0,
                Vector::new(sprite.dest.left, sprite.dest.top),
            ));
            sources.push(sprite.src);
        }
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        if !xforms.is_empty() {
            self.canvas.draw_atlas(
                image,
                &xforms,
                &sources,
                None,
                BlendMode::SrcOver,
                self.sampling,
                None,
                Some(&paint),
            );
        }
        for sprite in rest {
            self.canvas.draw_image_rect_with_sampling_options(
                image,
                Some((&sprite.src, SrcRectConstraint::Fast)),
                sprite.dest,
                self.sampling,
                &paint,
            );
        }
        self.canvas
    }

    pub fn draw_texts_batch(
        &self,
        style: &TextStyle,
        items: &[PlacedText],
    ) -> Result<usize, RenderError> {
        if items.is_empty() {
            return Ok(0);
        }
        let layout = self.text_layout(style)?;
        let font = layout.primary_font();
        let paint = text_paint(style.color);
        let cache = self.context.text_blobs();
        let bold = style.weight.is_bold();
        let mut drawn = 0usize;
        for item in items {
            let key = TextBlobKey::new(&style.family, style.size, bold, &item.text);
            let blob = match cache.get(&key) {
                Some(blob) => blob,
                None => match TextBlob::from_str(&item.text, font) {
                    Some(blob) => cache.insert(key, blob),
                    None => continue,
                },
            };
            self.canvas
                .draw_text_blob(&blob, (item.x.round(), item.y.round()), &paint);
            drawn += 1;
        }
        Ok(drawn)
    }

    pub fn text_blob_cache(&self) -> &'a TextBlobCache {
        self.context.text_blobs()
    }

    pub fn text_layout(&self, style: &TextStyle) -> Result<Arc<TextLayout>, RenderError> {
        let bold = style.weight.is_bold();
        let mut families = style.families();
        for family in self.chain.fallbacks() {
            if !families.contains(family) {
                families.push(family.clone());
            }
        }
        let size = style.size.max(1);
        let mut key = families.join("\u{1}");
        key.push('\u{2}');
        key.push_str(self.chain.primary());
        let key = (key, size, bold);
        let cache = self.context.layouts();
        if let Some(hit) = cache.get(&key) {
            return Ok(Arc::clone(hit.value()));
        }
        let mut fonts = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut last_error: Option<RenderError> = None;
        for family in &families {
            match self.fonts.typeface(family, bold) {
                Ok(typeface) => {
                    let name = typeface.family_name();
                    if !seen.insert(name) {
                        continue;
                    }
                    fonts.push(crisp_font(Font::new(typeface.duplicate(), size as f32)));
                }
                Err(error) => last_error = Some(error),
            }
        }
        if fonts.is_empty() {
            return Err(last_error.unwrap_or_else(|| {
                RenderError::Font(format!(
                    "字体链为空, 无法绘制文本 (请求字体族 {})",
                    style.family
                ))
            }));
        }
        let layout = Arc::new(TextLayout { fonts });
        if cache.len() > 512 {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&layout));
        Ok(layout)
    }

    pub fn measure(&self, text: &str, style: &TextStyle) -> Result<TextMetrics, RenderError> {
        let layout = self.text_layout(style)?;
        Ok(layout.metrics(text, style.letter_spacing))
    }

    pub fn draw_text(
        &self,
        text: &str,
        x: f32,
        y: f32,
        style: &TextStyle,
    ) -> Result<TextMetrics, RenderError> {
        let layout = self.text_layout(style)?;
        let metrics = layout.metrics(text, style.letter_spacing);
        let y = y.round();
        let mut cursor = match style.align {
            TextAlign::Left => x,
            TextAlign::Center => x - metrics.width / 2.0,
            TextAlign::Right => x - metrics.width,
        };
        let paint = text_paint(style.color);
        if self.draw_cached_text(&layout, text, cursor, y, style, &paint) {
            return Ok(metrics);
        }
        if style.letter_spacing == 0.0 {
            let mut run = String::new();
            let mut run_font = layout.primary_font();
            let flush = |canvas: &Canvas, run: &mut String, font: &Font, cursor: &mut f32| {
                if run.is_empty() {
                    return;
                }
                if let Some(blob) = TextBlob::from_str(run.as_str(), font) {
                    canvas.draw_text_blob(&blob, (*cursor, y), &paint);
                } else {
                    canvas.draw_str(run.as_str(), (*cursor, y), font, &paint);
                }
                *cursor += font.measure_str(run.as_str(), None).0;
                run.clear();
            };
            for ch in text.chars() {
                let font = layout.pick(ch);
                if !std::ptr::eq(font, run_font) {
                    flush(self.canvas, &mut run, run_font, &mut cursor);
                    run_font = font;
                }
                run.push(ch);
            }
            flush(self.canvas, &mut run, run_font, &mut cursor);
            return Ok(metrics);
        }
        for ch in text.chars() {
            let font = layout.pick(ch);
            let mut buffer = [0u8; 4];
            let piece: &str = ch.encode_utf8(&mut buffer);
            self.canvas.draw_str(piece, (cursor, y), font, &paint);
            cursor += font.measure_str(piece, None).0 + style.letter_spacing;
        }
        Ok(metrics)
    }

    fn draw_cached_text(
        &self,
        layout: &TextLayout,
        text: &str,
        x: f32,
        y: f32,
        style: &TextStyle,
        paint: &Paint,
    ) -> bool {
        if text.len() > TEXT_BLOB_MAX_BYTES
            || style.letter_spacing != 0.0
            || !layout.primary_covers(text)
        {
            return false;
        }
        let key = TextBlobKey::new(&style.family, style.size, style.weight.is_bold(), text);
        let cache = self.context.text_blobs();
        let blob = match cache.get(&key) {
            Some(hit) => hit,
            None => match TextBlob::from_str(text, layout.primary_font()) {
                Some(fresh) => cache.insert(key, fresh),
                None => return false,
            },
        };
        self.canvas.draw_text_blob(&blob, (x, y), paint);
        true
    }

    pub fn truncate_with_ellipsis(&self, text: &str, style: &TextStyle, max_width: f32) -> String {
        if max_width <= 0.0 {
            return String::new();
        }
        let Ok(layout) = self.text_layout(style) else {
            return String::new();
        };
        if layout.width(text, style.letter_spacing) <= max_width {
            return text.to_string();
        }
        let ellipsis_width = layout.width(ELLIPSIS, style.letter_spacing);
        if ellipsis_width > max_width {
            return String::new();
        }
        let mut kept = String::new();
        let mut width = 0.0;
        for ch in text.chars() {
            let advance = layout.advance(ch, style.letter_spacing);
            if width + advance + ellipsis_width > max_width {
                break;
            }
            kept.push(ch);
            width += advance;
        }
        kept.push_str(ELLIPSIS);
        kept
    }

    pub fn fit_to_width(
        &self,
        text: &str,
        style: &TextStyle,
        max_width: f32,
    ) -> (String, TextStyle) {
        self.fit_to_width_with(FitMode::Both, text, style, max_width)
    }

    pub fn fit_to_width_with(
        &self,
        mode: FitMode,
        text: &str,
        style: &TextStyle,
        max_width: f32,
    ) -> (String, TextStyle) {
        if max_width <= 0.0 {
            return (String::new(), style.clone());
        }
        let Ok(layout) = self.text_layout(style) else {
            return (text.to_string(), style.clone());
        };
        let full = layout.width(text, style.letter_spacing);
        if full <= max_width {
            return (text.to_string(), style.clone());
        }
        let mut narrowed = style.clone();
        if mode.shrinks() {
            let scale = max_width / full;
            let mut size = ((style.size as f32) * scale).floor() as u32;
            size = size.max(MIN_FONT_SIZE);
            let mut candidate = style.clone().with_size(size);
            while candidate.size > MIN_FONT_SIZE {
                let wide = self
                    .measure(text, &candidate)
                    .map(|metrics| metrics.width)
                    .unwrap_or(f32::INFINITY);
                if wide <= max_width {
                    break;
                }
                let next = candidate.size - 1;
                candidate = candidate.with_size(next);
            }
            let wide = self
                .measure(text, &candidate)
                .map(|metrics| metrics.width)
                .unwrap_or(f32::INFINITY);
            if wide <= max_width {
                return (text.to_string(), candidate);
            }
            narrowed = candidate;
        }
        if mode.truncates() {
            let shortened = self.truncate_with_ellipsis(text, &narrowed, max_width);
            return (shortened, narrowed);
        }
        (text.to_string(), narrowed)
    }

    pub fn wrap(&self, text: &str, style: &TextStyle, max_width: f32) -> Vec<String> {
        let Ok(layout) = self.text_layout(style) else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            if paragraph.is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut current = String::new();
            let mut width = 0.0;
            for ch in paragraph.chars() {
                let advance = layout.advance(ch, style.letter_spacing);
                if !current.is_empty() && width + advance > max_width {
                    lines.push(std::mem::take(&mut current));
                    width = 0.0;
                }
                current.push(ch);
                width += advance;
            }
            if !current.is_empty() {
                lines.push(current);
            }
        }
        lines
    }

    pub fn draw_paragraph(
        &self,
        text: &str,
        x: f32,
        y: f32,
        max_width: f32,
        line_height: f32,
        style: &TextStyle,
    ) -> Result<f32, RenderError> {
        let metrics = self.measure(text, style)?;
        let leading = if line_height > 0.0 {
            line_height
        } else {
            metrics.height
        };
        let mut baseline = y + metrics.baseline;
        for line in self.wrap(text, style, max_width) {
            self.draw_text(&line, x, baseline, style)?;
            baseline += leading;
        }
        Ok(leading)
    }

    pub fn badge_size(&self, text: &str, style: &TextStyle, padding: Padding) -> (f32, f32) {
        let metrics = self.measure(text, style).unwrap_or(TextMetrics {
            width: 0.0,
            height: 0.0,
            baseline: 0.0,
        });
        (
            metrics.width + padding.x * 2.0,
            metrics.height + padding.y * 2.0,
        )
    }

    pub fn badge(
        &self,
        text: &str,
        style: &TextStyle,
        background: Color,
        foreground: Color,
        padding: Padding,
    ) -> Result<Rect, RenderError> {
        self.badge_at(
            Point::new(0.0, 0.0),
            text,
            style,
            background,
            foreground,
            padding,
        )
    }

    pub fn badge_at(
        &self,
        origin: Point,
        text: &str,
        style: &TextStyle,
        background: Color,
        foreground: Color,
        padding: Padding,
    ) -> Result<Rect, RenderError> {
        let metrics = self.measure(text, style)?;
        let width = metrics.width + padding.x * 2.0;
        let height = metrics.height + padding.y * 2.0;
        let rect = Rect::from_xywh(origin.x, origin.y, width, height);
        let radius = height / 2.0;
        self.rrect_fill(rect, radius, background);
        let inner = style
            .clone()
            .with_color(foreground)
            .with_align(TextAlign::Left);
        let text_y = origin.y + padding.y + metrics.baseline;
        self.draw_text(text, origin.x + padding.x, text_y, &inner)?;
        Ok(rect)
    }
}

impl std::fmt::Debug for CanvasKit<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CanvasKit")
            .field("chain", &self.chain)
            .finish_non_exhaustive()
    }
}

pub fn rounded(rect: Rect, radius: f32) -> RRect {
    RRect::new_rect_xy(rect, radius.max(0.0), radius.max(0.0))
}

pub fn solid_paint(color: Color) -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(color.to_skia());
    paint
}

pub fn cover_source_rect(image: &Image, rect: Rect) -> Rect {
    let source_width = image.width() as f32;
    let source_height = image.height() as f32;
    if source_width <= 0.0 || source_height <= 0.0 {
        return Rect::from_xywh(0.0, 0.0, 0.0, 0.0);
    }
    let target_width = rect.width().abs().max(1.0);
    let target_height = rect.height().abs().max(1.0);
    let scale = (source_width / target_width).max(source_height / target_height);
    let width = (target_width * scale).min(source_width);
    let height = (target_height * scale).min(source_height);
    let left = (source_width - width) / 2.0;
    let top = (source_height - height) / 2.0;
    Rect::from_xywh(left, top, width, height)
}

pub fn weight_of(weight: FontWeight) -> u16 {
    weight.value()
}

pub fn contain_destination_rect(image: &Image, rect: Rect) -> Rect {
    let source_width = image.width() as f32;
    let source_height = image.height() as f32;
    if source_width <= 0.0 || source_height <= 0.0 {
        return rect;
    }
    let target_width = rect.width().abs().max(1.0);
    let target_height = rect.height().abs().max(1.0);
    let scale = (target_width / source_width).min(target_height / source_height);
    let width = source_width * scale;
    let height = source_height * scale;
    Rect::from_xywh(
        rect.left() + (rect.width() - width) / 2.0,
        rect.top() + (rect.height() - height) / 2.0,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap() -> Image {
        surfaces::raster_n32_premul((1, 1))
            .expect("测试位图应当可创建")
            .image_snapshot()
    }

    #[test]
    fn default_fallbacks_prefer_simplified_chinese() {
        let simplified = DEFAULT_FALLBACKS
            .iter()
            .position(|family| *family == "Noto Sans CJK SC")
            .expect("兜底链必须包含简体中文字族");
        let japanese = DEFAULT_FALLBACKS
            .iter()
            .position(|family| *family == "Noto Sans CJK JP")
            .expect("兜底链必须包含日文字族");
        assert!(
            simplified < japanese,
            "简体字族必须排在日文字族之前, 否则共用码位会取到日文字形"
        );
        assert_eq!(DEFAULT_FALLBACKS[0], "Noto Sans CJK SC");
    }

    #[test]
    fn scaled_cache_evicts_least_recently_used() {
        let mut store = ScaledStore::with_limits(2, u64::MAX);
        let image = bitmap();
        let keys = [
            (1u32, 0, 0, 0, 0, 0, 0),
            (2u32, 0, 0, 0, 0, 0, 0),
            (3u32, 0, 0, 0, 0, 0, 0),
        ];
        store.put(keys[0], image.clone(), None);
        store.put(keys[1], image.clone(), None);
        assert!(store.get(&keys[0]).is_some(), "已写入的条目应当可命中");
        store.put(keys[2], image, None);
        assert_eq!(store.len(), 2, "写入第三条后应当只保留两条");
        assert!(store.get(&keys[0]).is_some(), "刚命中过的条目不该被淘汰");
        assert!(store.get(&keys[1]).is_none(), "最久未用的条目应当先被淘汰");
        assert!(store.get(&keys[2]).is_some());
    }
}
