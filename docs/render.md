# 渲染

卡片在进程内渲染，输出 PNG / WebP / JPEG 字节流。
绘制与编码跑在独立渲染线程池，按优先级排队。
机器人的消息处理线程只负责提交任务和发消息，不会被绘制卡住。

## 画一张卡片

一次渲染就是「请求 + 绘制闭包」：请求定尺寸与格式，闭包拿 `kit` 画。

```rust
use aurorix_plugin::{Outgoing, RenderCard, ReplyKind};
use aurorix_render::skia_safe::Rect;
use aurorix_render::{Color, RenderFormat, RenderRequest, TextStyle};

let request = RenderRequest::new(900, 520)
    .with_format(RenderFormat::Png)
    .with_scale(2.0)                     // 逻辑 900x520，输出 1800x1040
    .with_background(Color::rgb(10, 12, 22));
let card = RenderCard::new(request, "card.png", |kit| {
    kit.fill_rect(Rect::from_xywh(0.0, 0.0, 900.0, 120.0), Color::rgb(24, 28, 48));
    kit.rrect_fill(Rect::from_xywh(40.0, 160.0, 820.0, 300.0), 18.0, Color::rgb(32, 38, 62));
    let style = TextStyle::new("Noto Sans CJK SC", 36).with_color(Color::WHITE);
    let _ = kit.draw_text("今日榜单", 72.0, 240.0, &style);
});
let reply = Outgoing::new(ReplyKind::Handler, "榜单").render(card);
```

闭包拿到的是逻辑坐标系，`with_scale` 只抬高输出分辨率，坐标不用重算。
`RenderCard::new` 只登记闭包，真正的绘制发生在渲染线程里，`reply` 照常走发送链路。

## 常用绘制

坐标统一用 `Rect::from_xywh(x, y, w, h)`，颜色用 `Color::rgb(r, g, b)` 或 `Color::new(r, g, b, a)`。

| 想做什么 | 怎么写 |
| --- | --- |
| 矩形 / 圆角矩形 | `kit.fill_rect(rect, color)` / `kit.rrect_fill(rect, radius, color)` |
| 渐变填充 | `kit.fill_gradient(rect, radius, &gradient)` |
| 画图片 | `kit.rounded_image(&image, rect, radius)` / `kit.cover_image(&image, rect)` |
| 一行文字 | `kit.draw_text(text, x, y, &style)` |
| 自动换行 | `kit.draw_paragraph(text, x, y, max_width, line_height, &style)` |
| 量宽 / 超宽省略 | `kit.measure(text, &style)`、`kit.truncate_with_ellipsis(text, &style, max_width)` |

插件素材不用自己解码，直接按 key 画：`kit.draw_asset_cover("covers/a.png", rect, 12.0)`。

## 文字与字体

```rust
let title = TextStyle::new("DejaVu Sans", 32).with_color(Color::WHITE);
let (fitted, fitted_style) = kit.fit_to_width("很长的一行标题", &title, 800.0);
let _ = kit.draw_text(&fitted, 48.0, 96.0, &fitted_style);

let body = TextStyle::new("DejaVu Sans", 20).with_color(Color::rgb(200, 205, 220));
let _ = kit.draw_paragraph("多行正文会自动折行……", 48.0, 150.0, 800.0, 32.0, &body);

let width = kit.measure("标题", &title).map_or(0.0, |m| m.width);
```

`TextStyle::new(family, size)` 的 `family` 是字体家族名，`draw_text` 的 `y` 是基线而不是顶边。
插件把 `.ttf` / `.otf` 放进 `assets/<id>/fonts/`，启动时按文件主干名注册，`Exo-Medium.ttf` 直接写 `"Exo-Medium"`。
缺字要回退时用 `FontChain::with_default_fallbacks("主字体").with_fallback("备选字体")`，按顺序取第一个含该字形的字体。

## 输出格式

格式在请求上定，`RenderFormat` 三个取值：

| 格式 | 什么时候用 |
| --- | --- |
| `Png` | 要清晰、要透明背景，卡片首选 |
| `WebP` | 体积小，群聊图多时省流量，支持透明 |
| `Jpeg` | 兼容最好，不支持透明，适合照片底图 |

质量默认 90，需要更高就 `.with_quality(95)`；不想记数字就用预设：`RenderQuality::HighWebp.apply(request)`，另有 `CrispPng` / `HighJpeg`，预设会同时定格式与编码参数。

## GPU

默认走 CPU 多线程光栅，不需要任何额外依赖。
想开硬件加速就带 `gpu` 特性编译：`cargo build --release --features gpu`。

运行时用环境变量 `AURORIX_GPU` 控制：`auto`（默认，探测到设备就用）、`on`、`off`。
没有可用显卡时自动回落到 CPU，不报错；渲染结果与 CPU 只差抗锯齿边缘。

## 并发

同一条回复里的多张卡片会并行渲染，互不排队。
线程数由 `config.json` 的 `render_pool` 控制，默认 4。
提交后立刻返回，结果回来再发消息。
