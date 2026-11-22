use std::collections::HashMap;

use aurorix_render::skia_safe::{image::CachingHint, AlphaType, ColorType, Data, Image, ImageInfo};

use crate::{pill_anchor, MAX_ROWS};

pub const TEXT_BAND: Band = Band::new(48, 64, 352, 44);

pub const BACKGROUND_BAND: Band = Band::new(48, 200, 804, 8);

pub const PILL_SCAN_WIDTH: usize = 140;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Band {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn scaled(self, scale: f32) -> Self {
        let scale = scale.max(0.0);
        Self {
            x: (self.x as f32 * scale) as usize,
            y: (self.y as f32 * scale) as usize,
            width: ((self.width as f32 * scale) as usize).max(2),
            height: ((self.height as f32 * scale) as usize).max(2),
        }
    }

    pub fn clamp_to(self, width: usize, height: usize) -> Self {
        let x = self.x.min(width.saturating_sub(2));
        let y = self.y.min(height.saturating_sub(2));
        Self {
            x,
            y,
            width: self.width.min(width.saturating_sub(x)).max(2),
            height: self.height.min(height.saturating_sub(y)).max(2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandStats {
    pub mean: f64,
    pub variance: f64,
    pub edge_delta: f64,
    pub edge_peak: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    pub width: usize,
    pub height: usize,
    pub distinct_colors: usize,
    pub text_band: BandStats,
    pub background_band: BandStats,
    pub pill_max_chroma: u8,
    pub pill_saturated_pixels: usize,
    pub pill_band: Band,
    pub top_colors: Vec<([u8; 3], usize)>,
}

pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let image = Image::from_encoded(Data::new_copy(bytes))?;
        let width = usize::try_from(image.width()).ok()?;
        let height = usize::try_from(image.height()).ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        let info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::RGBA8888,
            AlphaType::Unpremul,
            None,
        );
        let mut pixels = vec![0u8; width * height * 4];
        let row_bytes = width * 4;
        if !image.read_pixels(&info, &mut pixels, row_bytes, (0, 0), CachingHint::Allow) {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let index = (y.min(self.height - 1) * self.width + x.min(self.width - 1)) * 4;
        [
            self.pixels[index],
            self.pixels[index + 1],
            self.pixels[index + 2],
            self.pixels[index + 3],
        ]
    }

    pub fn luminance(&self, x: usize, y: usize) -> f64 {
        let pixel = self.pixel(x, y);
        0.2126 * f64::from(pixel[0]) + 0.7152 * f64::from(pixel[1]) + 0.0722 * f64::from(pixel[2])
    }

    pub fn band_stats(&self, band: Band) -> BandStats {
        let band = band.clamp_to(self.width, self.height);
        let mut sum = 0.0;
        let mut sum_squares = 0.0;
        let mut count = 0.0;
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut deltas: Vec<f64> = Vec::with_capacity(band.width * band.height);
        for y in band.y..band.y + band.height {
            for x in band.x..band.x + band.width {
                let value = self.luminance(x, y);
                sum += value;
                sum_squares += value * value;
                count += 1.0;
                min = min.min(value);
                max = max.max(value);
                if x + 1 < band.x + band.width {
                    deltas.push((self.luminance(x + 1, y) - value).abs());
                }
            }
        }
        let mean = if count > 0.0 { sum / count } else { 0.0 };
        let variance = if count > 0.0 {
            (sum_squares / count - mean * mean).max(0.0)
        } else {
            0.0
        };
        let edge_delta = if deltas.is_empty() {
            0.0
        } else {
            deltas.iter().sum::<f64>() / deltas.len() as f64
        };
        let edge_peak = if deltas.is_empty() {
            0.0
        } else {
            let mut sorted = deltas;
            sorted.sort_by(|left, right| {
                left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
            });
            let index = ((sorted.len() - 1) as f64 * 0.99) as usize;
            sorted[index]
        };
        BandStats {
            mean,
            variance,
            edge_delta,
            edge_peak,
            min,
            max,
        }
    }
}

pub fn pill_band(scale: f32) -> Band {
    let (x, y, height) = pill_anchor(0, MAX_ROWS);
    Band::new(
        x.max(0.0) as usize,
        y.max(0.0) as usize,
        PILL_SCAN_WIDTH,
        height.max(1.0) as usize,
    )
    .scaled(scale)
}

pub fn analyze(image: &RgbaImage, scale: f32) -> Analysis {
    let mut counts: HashMap<u32, u32> = HashMap::with_capacity(4096);
    for chunk in image.pixels.as_chunks::<4>().0 {
        let key = u32::from(chunk[0]) << 16 | u32::from(chunk[1]) << 8 | u32::from(chunk[2]);
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut top_colors: Vec<([u8; 3], usize)> = counts
        .iter()
        .map(|(key, count)| {
            (
                [(key >> 16) as u8, (key >> 8) as u8, (*key & 0xff) as u8],
                *count as usize,
            )
        })
        .collect();
    top_colors.sort_by_key(|left| std::cmp::Reverse(left.1));
    top_colors.truncate(5);

    let band = pill_band(scale);
    let band = band.clamp_to(image.width, image.height);
    let mut pill_max_chroma = 0u8;
    let mut pill_saturated_pixels = 0usize;
    for y in band.y..band.y + band.height {
        for x in band.x..band.x + band.width {
            let pixel = image.pixel(x, y);
            let high = pixel[0].max(pixel[1]).max(pixel[2]);
            let low = pixel[0].min(pixel[1]).min(pixel[2]);
            let chroma = high - low;
            pill_max_chroma = pill_max_chroma.max(chroma);
            if chroma > 60 {
                pill_saturated_pixels += 1;
            }
        }
    }

    Analysis {
        width: image.width,
        height: image.height,
        distinct_colors: counts.len(),
        text_band: image.band_stats(TEXT_BAND.scaled(scale)),
        background_band: image.band_stats(BACKGROUND_BAND.scaled(scale)),
        pill_max_chroma,
        pill_saturated_pixels,
        pill_band: band,
        top_colors,
    }
}
