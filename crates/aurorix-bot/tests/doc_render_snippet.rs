use aurorix_plugin::{Outgoing, RenderCard, ReplyKind};
use aurorix_render::skia_safe::Rect;
use aurorix_render::{Color, RenderFormat, RenderRequest, TextStyle};

#[test]
fn render_doc_snippet_compiles() {
    let request = RenderRequest::new(900, 520)
        .with_format(RenderFormat::Png)
        .with_scale(2.0)
        .with_background(Color::rgb(10, 12, 22));
    let card = RenderCard::new(request, "card.png", |kit| {
        kit.fill_rect(
            Rect::from_xywh(0.0, 0.0, 900.0, 120.0),
            Color::rgb(24, 28, 48),
        );
        kit.rrect_fill(
            Rect::from_xywh(40.0, 160.0, 820.0, 300.0),
            18.0,
            Color::rgb(32, 38, 62),
        );
        let style = TextStyle::new("Noto Sans CJK SC", 36).with_color(Color::WHITE);
        let _ = kit.draw_text("今日榜单", 72.0, 240.0, &style);
    });
    let reply = Outgoing::new(ReplyKind::Handler, "榜单").card(card);
    assert!(reply.has_render());
}
