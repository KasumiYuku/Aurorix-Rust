pub const DEFAULT_IMAGE_MAX_WIDTH: u32 = aurorix_core::DEFAULT_IMAGE_MAX_WIDTH;

pub const DEFAULT_IMAGE_MAX_HEIGHT: u32 = aurorix_core::DEFAULT_IMAGE_MAX_HEIGHT;

pub const MAX_IMAGE_SIDE: u32 = 1 << 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageBox {
    pub width: u32,
    pub height: u32,
}

impl Default for ImageBox {
    fn default() -> Self {
        Self::new(DEFAULT_IMAGE_MAX_WIDTH, DEFAULT_IMAGE_MAX_HEIGHT)
    }
}

impl ImageBox {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn off() -> Self {
        Self::new(0, 0)
    }

    pub const fn on(&self) -> bool {
        self.width > 0 || self.height > 0
    }

    pub fn limit(&self, w: u32, h: u32) -> (u32, u32) {
        if !self.on()
            || w == 0
            || h == 0
            || w > MAX_IMAGE_SIDE
            || h > MAX_IMAGE_SIDE
            || self.width > MAX_IMAGE_SIDE
            || self.height > MAX_IMAGE_SIDE
        {
            return (w, h);
        }
        let width = if self.width > 0 && w > self.width {
            self.width
        } else {
            w
        };
        let height = scaled(u64::from(h), u64::from(width), u64::from(w));
        if self.height > 0 && height > self.height {
            (width, self.height)
        } else {
            (width, height)
        }
    }
}

fn scaled(value: u64, numerator: u64, denominator: u64) -> u32 {
    let computed = (value * numerator + denominator / 2) / denominator;
    u32::try_from(computed)
        .unwrap_or(MAX_IMAGE_SIDE)
        .clamp(1, MAX_IMAGE_SIDE)
}

pub fn clamp_markdown(content: &str, image_box: &ImageBox) -> String {
    if !image_box.on() {
        return content.to_string();
    }
    crate::markdown::fit_images(content, image_box.width, image_box.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_350_438() -> ImageBox {
        ImageBox::new(350, 438)
    }

    #[test]
    fn default_box_matches_go_constants() {
        let default = ImageBox::default();
        assert_eq!(default.width, 350);
        assert_eq!(default.height, 438);
        assert!(default.on());
        assert!(!ImageBox::off().on());
        assert!(ImageBox::new(0, 100).on(), "任一方向有上限就算开启");
    }

    #[test]
    fn limit_only_shrinks_and_caps_both_sides() {
        assert_eq!(box_350_438().limit(1000, 2000), (350, 438));
        assert_eq!(box_350_438().limit(300, 200), (300, 200), "只缩不放");
        assert_eq!(box_350_438().limit(700, 438), (350, 219), "高按比例缩");
        assert_eq!(box_350_438().limit(700, 2000), (350, 438), "高被单独封顶");
        assert_eq!(ImageBox::off().limit(1000, 2000), (1000, 2000));
        assert_eq!(box_350_438().limit(0, 2000), (0, 2000), "零宽不参与");
    }

    #[test]
    fn scaled_height_rounds_to_nearest() {
        assert_eq!(box_350_438().limit(1000, 1251), (350, 438));
        assert_eq!(
            ImageBox::new(350, 100_000).limit(1000, 1251),
            (350, 438),
            "1251*350/1000 = 437.85 四舍五入成 438"
        );
        assert_eq!(
            ImageBox::new(350, 100_000).limit(1000, 1250),
            (350, 438),
            "1250*350/1000 = 437.5 进位成 438"
        );
        assert_eq!(
            ImageBox::new(350, 100_000).limit(1000, 1252),
            (350, 438),
            "438.2 舍成 438"
        );
        assert_eq!(
            ImageBox::new(350, 100_000).limit(1000, 1254),
            (350, 439),
            "438.9 进位成 439"
        );
    }

    #[test]
    fn limit_keeps_at_least_one_pixel() {
        let tall = ImageBox::new(1, MAX_IMAGE_SIDE);
        assert_eq!(tall.limit(1000, 1), (1, 1));
    }

    #[test]
    fn clamp_markdown_rewrites_annotated_images() {
        assert_eq!(
            clamp_markdown("![卡片 #1000px #2000px](https://e/a.png)", &box_350_438()),
            "![卡片 #350px #438px](https://e/a.png)"
        );
        assert_eq!(
            clamp_markdown("![卡片](https://e/a.png)", &box_350_438()),
            "![卡片](https://e/a.png)",
            "没有 #Wpx 的图不动"
        );
        assert_eq!(
            clamp_markdown("![卡片 #1000px #2000px](https://e/a.png)", &ImageBox::off()),
            "![卡片 #1000px #2000px](https://e/a.png)",
            "上限为 0 时原样返回"
        );
        assert_eq!(
            clamp_markdown("没有 px 字样 ![](https://e/a.png)", &box_350_438()),
            "没有 px 字样 ![](https://e/a.png)"
        );
    }

    #[test]
    fn clamp_markdown_handles_each_image_on_its_own() {
        let content = "前 ![a #1000px #2000px](https://e/a.png) 中 ![b #200px #100px](https://e/b.png) 后";
        assert_eq!(
            clamp_markdown(content, &box_350_438()),
            "前 ![a #350px #438px](https://e/a.png) 中 ![b #200px #100px](https://e/b.png) 后"
        );
        let twice = clamp_markdown(
            &clamp_markdown("![a #1000px #2000px](https://e/a.png)", &box_350_438()),
            &box_350_438(),
        );
        assert_eq!(
            twice, "![a #350px #438px](https://e/a.png)",
            "夹取要幂等"
        );
    }
}
