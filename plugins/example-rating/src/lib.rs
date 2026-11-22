use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use aurorix_buttons::{visited, Builder};
use aurorix_core::RoleRequired;
use aurorix_plugin::{
    ArgsSpec, CommandSpec, FlagSpec, Invocation, Outgoing, PluginCommands, PositionalSpec, Rate,
    RenderCard, ReplyKind,
};
use aurorix_render::skia_safe::{BlendMode, Image, Point, Rect};
use aurorix_render::{
    sampling_cubic, Border, CanvasKit, Color, FontChain, Gradient, GradientStop, LinearGradient,
    Padding, RadialGradient, RenderFormat, RenderRequest, TextAlign, TextStyle,
};

pub mod covers;
pub mod metrics;
pub mod sample;

pub use covers::{cover_image, COVER_SIZE};
pub use sample::{
    emit_sample_outputs, sample_output_dir, sample_output_dir_from, sample_output_paths,
    SampleOutput, SAMPLE_ENV, SAMPLE_PNG_NAME, SAMPLE_WEBP_NAME,
};

pub const CARD_WIDTH: i32 = 900;

pub const CARD_HEIGHT: i32 = 1200;

pub const CARD_SCALE: f32 = 2.0;

pub const CARD_MARGIN: f32 = 48.0;

pub const HEADER_HEIGHT: f32 = 196.0;

pub const ROWS_TOP: f32 = 210.0;

pub const ROWS_BOTTOM: f32 = 1050.0;

pub const ROW_GAP: f32 = 12.0;

pub const MAX_ROWS: usize = 10;

pub const MIN_ROW_HEIGHT: f32 = 56.0;

pub const MAX_ROW_HEIGHT: f32 = 96.0;

pub const FOOTER_TOP: f32 = 1074.0;

pub const FOOTER_HEIGHT: f32 = 64.0;

pub const PLUGIN_ID: &str = "rating";

pub const COMMAND: &str = "b30";

pub const DEFAULT_TOP: usize = 10;

pub const FONT_SANS: &str = "DejaVu Sans";

pub const FONT_MONO: &str = "DejaVu Sans Mono";

pub const COVER_ASSET_KEY: &str = "covers/star.png";

#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub title: String,
    pub difficulty: String,
    pub rating: f64,
}

impl Score {
    pub fn new(title: impl Into<String>, difficulty: impl Into<String>, rating: f64) -> Self {
        Self {
            title: title.into(),
            difficulty: difficulty.into(),
            rating,
        }
    }
}

pub trait Best30Store: Send + Sync {
    fn fetch(&self, user: &str) -> Vec<Score>;
}

#[derive(Debug, Default)]
pub struct MemoryStore {
    rows: Mutex<BTreeMap<String, Vec<Score>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&self, user: impl Into<String>, scores: Vec<Score>) -> &Self {
        self.rows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(user.into(), scores);
        self
    }
}

impl Best30Store for MemoryStore {
    fn fetch(&self, user: &str) -> Vec<Score> {
        self.rows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(user)
            .cloned()
            .unwrap_or_default()
    }
}

pub fn average_rating(scores: &[Score], top: usize) -> f64 {
    if scores.is_empty() || top == 0 {
        return 0.0;
    }
    let mut ratings: Vec<f64> = scores.iter().map(|score| score.rating).collect();
    ratings.sort_by(|left, right| right.total_cmp(left));
    let take = top.min(ratings.len());
    ratings.iter().take(take).sum::<f64>() / take as f64
}

pub fn card_font_chain() -> FontChain {
    FontChain::with_default_fallbacks(FONT_SANS)
        .with_fallback(FONT_MONO)
        .with_fallback("Noto Sans CJK JP")
        .with_fallback("Noto Sans CJK SC")
}

pub fn row_height(count: usize) -> f32 {
    let count = count.max(1) as f32;
    let available = ROWS_BOTTOM - ROWS_TOP;
    let raw = (available - ROW_GAP * (count - 1.0)) / count;
    raw.clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT)
}

pub fn row_rect(index: usize, count: usize) -> Rect {
    let height = row_height(count);
    let top = ROWS_TOP + index as f32 * (height + ROW_GAP);
    Rect::from_xywh(
        CARD_MARGIN,
        top,
        CARD_WIDTH as f32 - CARD_MARGIN * 2.0,
        height,
    )
}

pub fn pill_anchor(index: usize, count: usize) -> (f32, f32, f32) {
    pill_box(row_rect(index, count))
}

fn pill_box(rect: Rect) -> (f32, f32, f32) {
    let cover = (rect.height() * 0.74).max(30.0);
    let height = (rect.height() * 0.34).clamp(20.0, 28.0);
    (
        rect.left() + 16.0 + cover + 16.0,
        rect.top() + rect.height() * 0.56,
        height,
    )
}

pub fn plugin(store: Arc<dyn Best30Store>) -> PluginCommands {
    PluginCommands::new(PLUGIN_ID, "音游成绩卡")
        .describe("Best30 平均分与成绩卡片")
        .command(command_spec(store))
}

pub fn command_spec(store: Arc<dyn Best30Store>) -> CommandSpec {
    CommandSpec::new(COMMAND)
        .alias("best30")
        .role(RoleRequired::Member)
        .cooldown(Rate::per_window(2, 10))
        .describe("查询 Best30: /b30 [用户] [--top=10]")
        .with_args(
            ArgsSpec::new()
                .positional(PositionalSpec::optional("user").help("要查询的用户, 省略则查自己"))
                .flag(
                    FlagSpec::value("top")
                        .short('n')
                        .default_value("10")
                        .help("参与平均的前 N 首"),
                ),
        )
        .on(move |invocation: Invocation| {
            let store = Arc::clone(&store);
            async move {
                let user = query_user(&invocation);
                let top = query_top(&invocation);
                let scores = load_scores(store, &user).await;
                Ok(Some(reply(&user, top, scores)))
            }
        })
}

async fn load_scores(store: Arc<dyn Best30Store>, user: &str) -> Vec<Score> {
    store.fetch(user)
}

fn query_user(invocation: &Invocation) -> String {
    invocation
        .args
        .value("user")
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| invocation.user.clone())
}

fn query_top(invocation: &Invocation) -> usize {
    invocation
        .args
        .value("top")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_TOP)
        .clamp(1, 30)
}

fn reply(user: &str, top: usize, mut scores: Vec<Score>) -> Outgoing {
    if scores.is_empty() {
        return Outgoing::new(
            ReplyKind::Handler,
            format!("还没有 {user} 的成绩记录, 先打一首再来吧"),
        );
    }
    scores.sort_by(|left, right| right.rating.total_cmp(&left.rating));
    scores.truncate(top);
    let average = average_rating(&scores, top);
    let request = RenderRequest::new(CARD_WIDTH, CARD_HEIGHT)
        .with_format(RenderFormat::Png)
        .with_scale(CARD_SCALE)
        .with_background(Color::rgb(10, 12, 22));
    let painter_user = user.to_string();
    let card = RenderCard::new(request, "b30.png", move |kit| {
        draw_card(kit, &painter_user, &scores, average);
    });
    let keyboard = Builder::new()
        .scope(PLUGIN_ID)
        .callback(
            "refresh",
            "刷新",
            "refresh",
            vec![
                visited("已刷新"),
                aurorix_buttons::style(aurorix_buttons::ButtonStyle::Blue),
            ],
        )
        .build();
    let mut outgoing = Outgoing::new(
        ReplyKind::Handler,
        format!("{user} 的 Best{top} 平均 {average:.4}"),
    )
    .card(card);
    if let Some(keyboard) = keyboard {
        outgoing = outgoing.keyboard(keyboard);
    }
    outgoing
}

pub fn draw_card(kit: &CanvasKit<'_>, user: &str, scores: &[Score], average: f64) {
    draw_card_with_assets(kit, user, scores, average, None, None);
}

pub fn draw_card_with_assets(
    kit: &CanvasKit<'_>,
    user: &str,
    scores: &[Score],
    average: f64,
    cover_asset: Option<&str>,
    display_font: Option<&str>,
) {
    let width = CARD_WIDTH as f32;
    let height = CARD_HEIGHT as f32;
    paint_background(kit, width, height);
    paint_texture(kit, width);
    paint_header(kit, user, scores.len(), average, width, display_font);
    let rows = scores.len().min(MAX_ROWS);
    paint_rows(kit, &scores[..rows], cover_asset);
    paint_footer(kit, average, rows, width);
}

fn paint_background(kit: &CanvasKit<'_>, width: f32, height: f32) {
    let full = Rect::from_xywh(0.0, 0.0, width, height);
    let base = Gradient::Linear(
        LinearGradient::new([
            GradientStop::new(0.0, Color::rgb(9, 11, 24)),
            GradientStop::new(0.32, Color::rgb(21, 23, 47)),
            GradientStop::new(0.66, Color::rgb(35, 22, 57)),
            GradientStop::new(1.0, Color::rgb(8, 9, 17)),
        ])
        .with_angle(112.0),
    );
    let _ = kit.fill_gradient(full, 0.0, &base);
    let glow = Gradient::Radial(
        RadialGradient::new([
            GradientStop::new(0.0, Color::new(96, 168, 255, 104)),
            GradientStop::new(0.42, Color::new(126, 96, 226, 52)),
            GradientStop::new(1.0, Color::new(10, 10, 20, 0)),
        ])
        .with_center(Point::new(width * 0.76, 26.0))
        .with_radius(width * 0.82),
    );
    let _ = kit.fill_gradient(full, 0.0, &glow);
    let vignette = Gradient::Radial(
        RadialGradient::new([
            GradientStop::new(0.0, Color::new(0, 0, 0, 0)),
            GradientStop::new(0.58, Color::new(0, 0, 0, 34)),
            GradientStop::new(1.0, Color::new(0, 0, 0, 168)),
        ])
        .with_center(Point::new(width * 0.5, height * 0.52))
        .with_radius(width * 0.96),
    );
    let _ = kit.fill_gradient(full, 0.0, &vignette);
}

fn paint_texture(kit: &CanvasKit<'_>, width: f32) {
    let strip = Rect::from_xywh(0.0, 0.0, width, HEADER_HEIGHT);
    kit.layer(strip, 0.32, BlendMode::Multiply, |kit| {
        let mut x = 6.0;
        while x < width {
            kit.fill_rect(
                Rect::from_xywh(x, 0.0, 1.5, HEADER_HEIGHT),
                Color::new(186, 202, 238, 150),
            );
            x += 23.0;
        }
        kit.fill_rect(
            Rect::from_xywh(0.0, HEADER_HEIGHT - 1.0, width, 1.0),
            Color::new(150, 176, 226, 120),
        );
    });
}

fn paint_header(
    kit: &CanvasKit<'_>,
    user: &str,
    count: usize,
    average: f64,
    width: f32,
    display_font: Option<&str>,
) {
    let accent = accent_gradient();
    let _ = kit.fill_gradient(Rect::from_xywh(0.0, 0.0, width, 8.0), 0.0, &accent);
    let title_font = display_font.unwrap_or(FONT_SANS);
    let title = TextStyle::new(title_font, 62)
        .with_bold(true)
        .with_letter_spacing(1.5)
        .with_color(Color::rgb(244, 247, 255));
    let measured = kit
        .measure("BEST 30", &title)
        .map(|m| m.width)
        .unwrap_or(240.0);
    let _ = kit.draw_text("BEST 30", CARD_MARGIN, 108.0, &title);

    let tag_style = TextStyle::new(FONT_SANS, 15)
        .with_bold(true)
        .with_letter_spacing(0.6)
        .with_color(Color::rgb(198, 222, 255));
    let tag_origin = Point::new(CARD_MARGIN + measured + 16.0, 72.0);
    let _ = kit.badge_at(
        tag_origin,
        "2× HiDPI",
        &tag_style,
        Color::new(96, 168, 255, 62),
        Color::rgb(198, 222, 255),
        Padding::new(12.0, 5.0),
    );

    let subtitle = TextStyle::new(FONT_SANS, 26).with_color(Color::rgb(152, 162, 198));
    let _ = kit.draw_text("音游成绩 · 个人最佳 30 首", CARD_MARGIN, 152.0, &subtitle);

    let meta = TextStyle::new(FONT_MONO, 15)
        .with_letter_spacing(3.0)
        .with_color(Color::rgb(126, 138, 174));
    let _ = kit.draw_text(
        &format!("TOP {count} / SEASON 04 / AURORIX"),
        CARD_MARGIN,
        184.0,
        &meta,
    );

    let player_style = TextStyle::new(FONT_SANS, 22)
        .with_color(Color::rgb(214, 222, 245))
        .with_align(TextAlign::Right);
    let (player, player_style) = kit.fit_to_width(user, &player_style, 280.0);
    let _ = kit.draw_text(&player, width - CARD_MARGIN, 184.0, &player_style);

    let badge_text = format!("AVG {average:.4}");
    let badge_style = TextStyle::new(FONT_MONO, 26)
        .with_bold(true)
        .with_color(Color::rgb(10, 12, 22));
    let padding = Padding::new(26.0, 14.0);
    let (badge_width, badge_height) = kit.badge_size(&badge_text, &badge_style, padding);
    let badge_rect = Rect::from_xywh(
        width - CARD_MARGIN - badge_width,
        54.0,
        badge_width,
        badge_height,
    );
    let _ = kit.pill(
        badge_rect,
        &Gradient::Linear(LinearGradient::horizontal([
            GradientStop::new(0.0, Color::rgb(88, 200, 250)),
            GradientStop::new(0.55, Color::rgb(150, 132, 255)),
            GradientStop::new(1.0, Color::rgb(244, 116, 208)),
        ])),
        Some(Border::new(Color::new(255, 255, 255, 84), 1.4)),
    );
    if let Ok(metrics) = kit.measure(&badge_text, &badge_style) {
        let _ = kit.draw_text(
            &badge_text,
            badge_rect.left() + padding.x,
            badge_rect.top() + padding.y + metrics.baseline,
            &badge_style,
        );
    }
    kit.circle(
        Point::new(badge_rect.left() + 13.0, badge_rect.bottom() + 14.0),
        3.5,
        Color::rgb(120, 236, 200),
    );
}

fn paint_rows(kit: &CanvasKit<'_>, scores: &[Score], cover_asset: Option<&str>) {
    let count = scores.len();
    let covers: Vec<Option<Image>> = (0..count).map(covers::cover_image).collect();
    let mut lowest = f64::INFINITY;
    let mut highest = f64::NEG_INFINITY;
    for score in scores {
        lowest = lowest.min(score.rating);
        highest = highest.max(score.rating);
    }
    let spread = (highest - lowest).max(0.0001);
    for (index, score) in scores.iter().enumerate() {
        let rect = row_rect(index, count);
        let cover = covers.get(index).and_then(|image| image.as_ref());
        let asset = if index == 0 { cover_asset } else { None };
        let ratio = 0.35 + 0.65 * ((score.rating - lowest) / spread);
        paint_row(kit, rect, score, index, cover, asset, ratio as f32);
    }
}

fn paint_row(
    kit: &CanvasKit<'_>,
    rect: Rect,
    score: &Score,
    index: usize,
    cover: Option<&Image>,
    cover_asset: Option<&str>,
    ratio: f32,
) {
    kit.drop_shadow_in(rect, Color::new(2, 4, 12, 168), 0.0, 7.0, 11.0, |kit| {
        let panel = Gradient::Linear(LinearGradient::horizontal([
            GradientStop::new(0.0, Color::rgb(23, 27, 45)),
            GradientStop::new(0.55, Color::rgb(27, 29, 51)),
            GradientStop::new(1.0, Color::rgb(39, 30, 58)),
        ]));
        let _ = kit.fill_gradient(rect, 18.0, &panel);
        kit.stroke(rect, 18.0, Border::new(Color::new(130, 158, 226, 30), 1.0));
        let stripe = Gradient::Linear(LinearGradient::vertical([
            GradientStop::new(0.0, Color::rgb(96, 204, 250)),
            GradientStop::new(1.0, Color::rgb(176, 120, 255)),
        ]));
        let _ = kit.fill_gradient(
            Rect::from_xywh(
                rect.left() + 0.0,
                rect.top() + 12.0,
                4.0,
                rect.height() - 24.0,
            ),
            2.0,
            &stripe,
        );

        let cover_size = (rect.height() * 0.74).max(30.0);
        let cover_rect = Rect::from_xywh(
            rect.left() + 16.0,
            rect.top() + (rect.height() - cover_size) / 2.0,
            cover_size,
            cover_size,
        );
        let from_asset = cover_asset
            .map(|key| kit.draw_asset_cover(key, cover_rect, 13.0).is_ok())
            .unwrap_or(false);
        if !from_asset {
            match cover {
                Some(image) => {
                    kit.rounded_image_with_sampling(image, cover_rect, 13.0, sampling_cubic());
                }
                None => {
                    kit.rrect_fill(cover_rect, 13.0, Color::rgb(48, 54, 84));
                }
            }
        }
        kit.stroke(
            cover_rect,
            13.0,
            Border::new(Color::new(255, 255, 255, 46), 1.0),
        );

        let rank_style = TextStyle::new(FONT_SANS, 13)
            .with_bold(true)
            .with_color(Color::WHITE);
        let _ = kit.badge_at(
            Point::new(cover_rect.left() + 4.0, cover_rect.top() + 4.0),
            &format!("#{}", index + 1),
            &rank_style,
            Color::new(6, 8, 16, 205),
            Color::WHITE,
            Padding::new(7.0, 3.0),
        );

        let text_x = cover_rect.right() + 16.0;
        let score_size = (rect.height() * 0.40).clamp(18.0, 32.0) as u32;
        let score_style = TextStyle::new(FONT_MONO, score_size)
            .with_bold(true)
            .with_color(Color::rgb(255, 214, 132))
            .with_align(TextAlign::Right);
        let score_text = format!("{:.4}", score.rating);
        let score_width = kit
            .measure(&score_text, &score_style)
            .map(|metrics| metrics.width)
            .unwrap_or(120.0);
        let score_right = rect.right() - 20.0;
        let title_max = (score_right - score_width - 18.0 - text_x).max(60.0);
        let title_style =
            TextStyle::new(FONT_SANS, (rect.height() * 0.38).clamp(17.0, 30.0) as u32)
                .with_bold(true)
                .with_color(Color::rgb(236, 241, 255));
        let (title, title_style) = kit.fit_to_width(&score.title, &title_style, title_max);
        let _ = kit.draw_text(
            &title,
            text_x,
            rect.top() + rect.height() * 0.44,
            &title_style,
        );

        let (light, deep) = difficulty_colors(&score.difficulty);
        let code = difficulty_code(&score.difficulty);
        let pill_style = TextStyle::new(FONT_SANS, (rect.height() * 0.22).clamp(11.0, 17.0) as u32)
            .with_bold(true)
            .with_color(Color::rgb(10, 12, 24));
        let code_width = kit
            .measure(&code, &pill_style)
            .map(|metrics| metrics.width)
            .unwrap_or(26.0);
        let (pill_x, pill_y, pill_height) = pill_box(rect);
        let pill = Rect::from_xywh(pill_x, pill_y, code_width + 22.0, pill_height);
        let _ = kit.pill(
            pill,
            &Gradient::Linear(LinearGradient::horizontal([
                GradientStop::new(0.0, light),
                GradientStop::new(1.0, deep),
            ])),
            Some(Border::new(Color::new(255, 255, 255, 64), 1.0)),
        );
        if let Ok(metrics) = kit.measure(&code, &pill_style) {
            let _ = kit.draw_text(
                &code,
                pill.center_x(),
                pill.center_y() - metrics.height / 2.0 + metrics.baseline,
                &pill_style.clone().with_align(TextAlign::Center),
            );
        }

        let label_style =
            TextStyle::new(FONT_SANS, (rect.height() * 0.20).clamp(11.0, 16.0) as u32)
                .with_color(Color::rgb(150, 160, 194));
        let label_x = pill.right() + 10.0;
        let label_max = (score_right - score_width - 16.0 - label_x).max(0.0);
        let label = kit.truncate_with_ellipsis(&score.difficulty, &label_style, label_max);
        let mut label_right = label_x;
        if let Ok(metrics) = kit.measure(&label, &label_style) {
            let _ = kit.draw_text(
                &label,
                label_x,
                pill.center_y() - metrics.height / 2.0 + metrics.baseline,
                &label_style,
            );
            label_right = label_x + metrics.width;
        }
        paint_gauge(
            kit,
            label_right + 16.0,
            score_right - score_width - 18.0,
            pill.center_y(),
            ratio,
        );

        let score_baseline = rect.top() + rect.height() * 0.46;
        let _ = kit.draw_text(&score_text, score_right, score_baseline, &score_style);
        let unit_style = TextStyle::new(FONT_MONO, 12)
            .with_letter_spacing(1.8)
            .with_color(Color::rgb(118, 128, 160))
            .with_align(TextAlign::Right);
        let _ = kit.draw_text("RATING", score_right, score_baseline + 15.0, &unit_style);
    });
}

fn paint_gauge(kit: &CanvasKit<'_>, from: f32, to: f32, center_y: f32, ratio: f32) {
    let width = to - from;
    if width < 90.0 {
        return;
    }
    let height = 6.0;
    kit.rrect_fill(
        Rect::from_xywh(from, center_y - height / 2.0, width, height),
        height / 2.0,
        Color::new(6, 8, 18, 130),
    );
    let filled = (width * ratio.clamp(0.0, 1.0)).max(height);
    let _ = kit.fill_gradient(
        Rect::from_xywh(from, center_y - height / 2.0, filled, height),
        height / 2.0,
        &accent_gradient(),
    );
}

fn paint_footer(kit: &CanvasKit<'_>, average: f64, rows: usize, width: f32) {
    let band = Rect::from_xywh(
        CARD_MARGIN,
        FOOTER_TOP,
        width - CARD_MARGIN * 2.0,
        FOOTER_HEIGHT,
    );
    let shade = Gradient::Linear(LinearGradient::horizontal([
        GradientStop::new(0.0, Color::rgb(52, 60, 96)),
        GradientStop::new(0.55, Color::rgb(38, 42, 70)),
        GradientStop::new(1.0, Color::rgb(30, 32, 50)),
    ]));
    let _ = kit.fill_gradient(band, 28.0, &shade);
    kit.stroke(band, 28.0, Border::new(Color::new(255, 255, 255, 34), 1.0));
    kit.layer(band, 0.42, BlendMode::Multiply, |kit| {
        let mut x = band.left() + 10.0;
        while x < band.right() {
            kit.fill_rect(
                Rect::from_xywh(x, band.top(), 2.0, band.height()),
                Color::new(196, 210, 246, 160),
            );
            x += 12.0;
        }
    });

    let pill_style = TextStyle::new(FONT_SANS, 19)
        .with_bold(true)
        .with_color(Color::rgb(10, 12, 22));
    let label = format!("TOP {rows}");
    let label_width = kit
        .measure(&label, &pill_style)
        .map(|metrics| metrics.width)
        .unwrap_or(48.0);
    let pill = Rect::from_xywh(
        band.left() + 14.0,
        band.center_y() - 17.0,
        label_width + 26.0,
        34.0,
    );
    let _ = kit.pill(
        pill,
        &accent_gradient(),
        Some(Border::new(Color::new(255, 255, 255, 92), 1.2)),
    );
    if let Ok(metrics) = kit.measure(&label, &pill_style) {
        let _ = kit.draw_text(
            &label,
            pill.center_x(),
            pill.center_y() - metrics.height / 2.0 + metrics.baseline,
            &pill_style.clone().with_align(TextAlign::Center),
        );
    }

    let caption_style = TextStyle::new(FONT_SANS, 17).with_color(Color::rgb(176, 186, 216));
    let _ = kit.draw_text(
        "平均潜力值",
        pill.right() + 14.0,
        band.center_y() + 6.0,
        &caption_style,
    );

    let total_style = TextStyle::new(FONT_MONO, 24)
        .with_bold(true)
        .with_color(Color::rgb(255, 214, 132))
        .with_align(TextAlign::Right);
    let _ = kit.draw_text(
        &format!("AVG {average:.4}"),
        band.right() - 18.0,
        band.center_y() + 8.0,
        &total_style,
    );

    let note = TextStyle::new(FONT_SANS, 14).with_color(Color::rgb(114, 124, 158));
    let _ = kit.draw_paragraph(
        "由 Aurorix-Rust 渲染引擎以 2 倍缩放绘制 · 圆角贴图 / 多段渐变 / 投影 / 混合图层 / CJK 字体回退",
        CARD_MARGIN,
        FOOTER_TOP + FOOTER_HEIGHT + 14.0,
        width - CARD_MARGIN * 2.0,
        22.0,
        &note,
    );
}

fn accent_gradient() -> Gradient {
    Gradient::Linear(LinearGradient::horizontal([
        GradientStop::new(0.0, Color::rgb(88, 200, 250)),
        GradientStop::new(0.5, Color::rgb(150, 132, 255)),
        GradientStop::new(1.0, Color::rgb(244, 116, 208)),
    ]))
}

fn difficulty_colors(difficulty: &str) -> (Color, Color) {
    let class = difficulty
        .split_whitespace()
        .next()
        .unwrap_or("Future")
        .to_ascii_lowercase();
    match class.as_str() {
        "beyond" => (Color::rgb(255, 138, 138), Color::rgb(214, 32, 84)),
        "present" => (Color::rgb(140, 244, 188), Color::rgb(16, 138, 104)),
        "past" => (Color::rgb(132, 208, 255), Color::rgb(30, 108, 196)),
        "eternal" => (Color::rgb(255, 226, 148), Color::rgb(198, 128, 24)),
        _ => (Color::rgb(196, 140, 255), Color::rgb(96, 52, 208)),
    }
}

fn difficulty_code(difficulty: &str) -> String {
    let class = difficulty.split_whitespace().next().unwrap_or("Future");
    let letters: String = class
        .chars()
        .filter(|value| value.is_ascii_alphabetic())
        .take(3)
        .collect();
    if letters.len() == 3 {
        letters.to_ascii_uppercase()
    } else {
        class
            .chars()
            .take(3)
            .collect::<String>()
            .to_ascii_uppercase()
    }
}

pub fn demo_scores() -> Vec<Score> {
    vec![
        Score::new("Grievous Lady", "Future 11", 13.42),
        Score::new("Fracture Ray", "Future 12", 13.18),
        Score::new("Tempestissimo", "Beyond 12", 12.96),
        Score::new("世界の終わり", "Future 10", 12.74),
        Score::new("Sheriruth", "Future 10", 12.51),
        Score::new("Ringed Genesis", "Past 9", 12.33),
        Score::new("Pentiment", "Beyond 11", 12.20),
        Score::new("Arcahv", "Present 8", 12.08),
        Score::new("アイウエオ ★♪", "Future 9", 11.94),
        Score::new("SAIKYO STRONGER", "Eternal 11", 11.80),
    ]
}
