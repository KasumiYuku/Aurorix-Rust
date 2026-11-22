use aurorix_render::skia_safe::{
    gradient, paint::Style, shaders, surfaces, Canvas, Color as SkColor, Color4f, Image, Paint,
    Point, Rect, TileMode,
};

pub const COVER_SIZE: i32 = 256;

const RING_COUNT: usize = 5;

const SPOKE_COUNT: usize = 11;

const PALETTE: [([u8; 3], [u8; 3]); 10] = [
    ([96, 172, 255], [26, 36, 104]),
    ([255, 138, 104], [122, 36, 78]),
    ([118, 236, 190], [18, 78, 104]),
    ([188, 122, 255], [62, 34, 128]),
    ([255, 214, 122], [150, 74, 36]),
    ([255, 108, 158], [104, 24, 84]),
    ([104, 238, 255], [14, 62, 118]),
    ([196, 255, 138], [44, 100, 56]),
    ([255, 176, 88], [118, 46, 28]),
    ([158, 168, 255], [44, 54, 130]),
];

pub fn cover_image(index: usize) -> Option<Image> {
    let mut surface = surfaces::raster_n32_premul((COVER_SIZE, COVER_SIZE))?;
    let size = COVER_SIZE as f32;
    let (light, deep) = PALETTE[index % PALETTE.len()];
    {
        let canvas = surface.canvas();
        paint_backdrop(canvas, size, light, deep);
        paint_rings(canvas, size, index);
        paint_spokes(canvas, size, index);
        paint_marks(canvas, size, index);
    }
    Some(surface.image_snapshot())
}

fn solid(alpha: u8, channels: [u8; 3]) -> Color4f {
    Color4f::from(SkColor::from_argb(
        alpha,
        channels[0],
        channels[1],
        channels[2],
    ))
}

fn paint_backdrop(canvas: &Canvas, size: f32, light: [u8; 3], deep: [u8; 3]) {
    let shade = [deep[0] / 2 + 6, deep[1] / 2 + 6, deep[2] / 2 + 12];
    let colors = [solid(255, light), solid(255, deep), solid(255, shade)];
    let positions = [0.0f32, 0.62, 1.0];
    let spec = gradient::Gradient::new(
        gradient::Colors::new(&colors, Some(&positions), TileMode::Clamp, None),
        gradient::Interpolation::default(),
    );
    let stripe = (Point::new(0.0, 0.0), Point::new(size, size));
    let Some(shader) = shaders::linear_gradient(stripe, &spec, None) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_shader(shader);
    canvas.draw_rect(Rect::from_xywh(0.0, 0.0, size, size), &paint);
}

fn paint_rings(canvas: &Canvas, size: f32, index: usize) {
    let center = Point::new(size * 0.37, size * 0.63);
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(Style::Stroke);
    for step in 0..RING_COUNT {
        let radius = size * (0.17 + step as f32 * 0.132);
        let width = size * (0.012 + step as f32 * 0.005);
        let alpha = 168u8.saturating_sub(step as u8 * 26);
        paint.set_stroke_width(width);
        paint.set_color(SkColor::from_argb(alpha, 255, 255, 255));
        canvas.draw_circle(center, radius, &paint);
    }
    let mut tint = Paint::default();
    tint.set_anti_alias(true);
    tint.set_style(Style::Stroke);
    tint.set_stroke_width(size * 0.02);
    tint.set_color(SkColor::from_argb(90, 255, 236, 200));
    let oval = Rect::from_xywh(size * 0.06, size * 0.30, size * 0.62, size * 0.62);
    canvas.draw_arc(oval, index as f32 * 11.0, 210.0, false, &tint);
}

fn paint_spokes(canvas: &Canvas, size: f32, index: usize) {
    let center = Point::new(size * 0.37, size * 0.63);
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(Style::Stroke);
    paint.set_stroke_width(size * 0.008);
    paint.set_color(SkColor::from_argb(78, 255, 255, 255));
    for spoke in 0..SPOKE_COUNT {
        let angle = std::f32::consts::TAU * spoke as f32 / SPOKE_COUNT as f32 + index as f32 * 0.21;
        let reach = size * if spoke % 2 == 0 { 0.46 } else { 0.31 };
        let tip = Point::new(
            center.x + angle.cos() * reach,
            center.y + angle.sin() * reach,
        );
        let root = Point::new(
            center.x + angle.cos() * size * 0.09,
            center.y + angle.sin() * size * 0.09,
        );
        canvas.draw_line(root, tip, &paint);
    }
}

fn paint_marks(canvas: &Canvas, size: f32, index: usize) {
    let sun = Point::new(size * 0.73, size * 0.25);
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(SkColor::from_argb(210, 255, 252, 238));
    canvas.draw_circle(sun, size * 0.093, &paint);
    paint.set_color(SkColor::from_argb(46, 8, 12, 26));
    for line in 0..4 {
        let top = size * (0.70 + line as f32 * 0.062) + index as f32 * 0.5;
        canvas.draw_rect(Rect::from_xywh(0.0, top, size, size * 0.022), &paint);
    }
}
