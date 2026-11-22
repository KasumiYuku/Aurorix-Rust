use crate::error::ButtonResult;
use crate::keyboard::{
    qualify, AllowedPermission, Button, ButtonStyle, Keyboard, DEFAULT_UNSUPPORTED_TIP,
    FONT_SIZE_SMALL, MAX_BUTTONS_PER_ROW, MAX_ROWS,
};

pub struct Opt(Box<dyn FnOnce(&mut Button) + Send>);

impl Opt {
    pub fn apply(self, button: &mut Button) {
        (self.0)(button);
    }
}

pub fn style(style: ButtonStyle) -> Opt {
    Opt(Box::new(move |button| button.render_data.style = style))
}

pub fn visited(label: impl Into<String>) -> Opt {
    let label = label.into();
    Opt(Box::new(move |button| {
        button.render_data.visited_label = label
    }))
}

pub fn only(users: Vec<String>) -> Opt {
    Opt(Box::new(move |button| {
        button.set_user_white_list(&users);
    }))
}

pub fn auto_send() -> Opt {
    Opt(Box::new(|button| button.action.auto_send = true))
}

pub fn tip(text: impl Into<String>) -> Opt {
    let text = text.into();
    Opt(Box::new(move |button| {
        button.action.unsupport_tips = Some(text)
    }))
}

pub struct Builder {
    keyboard: Keyboard,
    scope: String,
    row: i32,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Self {
        Self {
            keyboard: Keyboard::new(),
            scope: String::new(),
            row: 0,
        }
    }

    pub fn scope(mut self, plugin_id: impl Into<String>) -> Self {
        self.scope = plugin_id.into();
        self
    }

    pub fn row(mut self) -> Self {
        self.row += 1;
        self
    }

    pub fn font_size(mut self, size: &str) -> Self {
        self.keyboard.set_font_size(size);
        self
    }

    pub fn small_font(self) -> Self {
        self.font_size(FONT_SIZE_SMALL)
    }

    pub fn link(self, id: &str, label: &str, url: &str, opts: Vec<Opt>) -> Self {
        self.add(
            id,
            label,
            ButtonStyle::Blue,
            Box::new({
                let url = url.to_string();
                move |button: &mut Button| {
                    button.set_href(&url);
                }
            }),
            opts,
        )
    }

    pub fn callback(self, id: &str, label: &str, data: &str, opts: Vec<Opt>) -> Self {
        self.add(
            id,
            label,
            ButtonStyle::Blue,
            Box::new({
                let data = data.to_string();
                move |button: &mut Button| {
                    button.set_callback(&data);
                }
            }),
            opts,
        )
    }

    pub fn command(self, id: &str, label: &str, content: &str, opts: Vec<Opt>) -> Self {
        self.add(
            id,
            label,
            ButtonStyle::Gray,
            Box::new({
                let content = content.to_string();
                move |button: &mut Button| {
                    button.set_auto_command(&content, false, false);
                }
            }),
            opts,
        )
    }

    pub fn build(self) -> Option<Keyboard> {
        if self.keyboard.rows.is_empty() {
            return None;
        }
        Some(self.keyboard)
    }

    pub fn keyboard(&self) -> &Keyboard {
        &self.keyboard
    }

    fn add(
        mut self,
        id: &str,
        label: &str,
        style: ButtonStyle,
        apply: Box<dyn FnOnce(&mut Button)>,
        opts: Vec<Opt>,
    ) -> Self {
        let Some(row) = self.slot(id) else {
            return self;
        };
        let qualified = qualify(&self.scope, id);
        let appended: ButtonResult<()> = match self
            .keyboard
            .append_button(&qualified, label, label, style, row)
        {
            Ok(button) => {
                button.set_user_white_list(&[]);
                button.set_unsupported_tip(DEFAULT_UNSUPPORTED_TIP);
                apply(button);
                for opt in opts {
                    opt.apply(button);
                }
                Ok(())
            }
            Err(err) => Err(err),
        };
        let _ = appended;
        self
    }

    fn slot(&mut self, _id: &str) -> Option<i32> {
        while self.row < MAX_ROWS as i32 {
            let row = self.row as usize;
            while self.keyboard.rows.len() <= row {
                self.keyboard.rows.push(Default::default());
            }
            if self.keyboard.rows[row].buttons.len() < MAX_BUTTONS_PER_ROW {
                return Some(self.row);
            }
            self.row += 1;
        }
        None
    }
}

pub fn all_users() -> AllowedPermission {
    AllowedPermission::AllUser
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_prefixes_button_ids() {
        let keyboard = Builder::new()
            .scope("arcaea")
            .command("refresh", "刷新", "/a b50", Vec::new())
            .build()
            .expect("keyboard");
        assert_eq!(keyboard.rows[0].buttons[0].id, "arcaea.refresh");
    }

    #[test]
    fn defaults_match_go_builder() {
        let keyboard = Builder::new()
            .command("b50", "Best 50", "/p b50", Vec::new())
            .callback("pick", "选择", "song:1", Vec::new())
            .link("site", "官网", "https://x", Vec::new())
            .build()
            .expect("keyboard");
        let buttons = &keyboard.rows[0].buttons;
        assert_eq!(buttons.len(), 3);
        assert_eq!(buttons[0].action.action_type, crate::ButtonAction::Command);
        assert_eq!(buttons[1].action.action_type, crate::ButtonAction::Callback);
        assert_eq!(buttons[2].action.action_type, crate::ButtonAction::Link);
        assert_eq!(buttons[0].render_data.style, ButtonStyle::Gray);
        assert_eq!(buttons[1].render_data.style, ButtonStyle::Blue);
        assert_eq!(buttons[2].render_data.style, ButtonStyle::Blue);
        for button in buttons {
            assert_eq!(button.render_data.visited_label, button.render_data.label);
            assert_eq!(
                button.action.permission.permission_type,
                AllowedPermission::AllUser
            );
            assert_eq!(
                button.action.unsupport_tips.as_deref(),
                Some(DEFAULT_UNSUPPORTED_TIP)
            );
        }
    }

    #[test]
    fn options_override_defaults() {
        let keyboard = Builder::new()
            .command(
                "confirm",
                "确认",
                "x",
                vec![
                    style(ButtonStyle::Blue),
                    visited("已确认"),
                    only(vec!["u1".to_string()]),
                    auto_send(),
                    tip("不支持"),
                ],
            )
            .build()
            .expect("keyboard");
        let button = &keyboard.rows[0].buttons[0];
        assert_eq!(button.render_data.style, ButtonStyle::Blue);
        assert_eq!(button.render_data.visited_label, "已确认");
        assert_eq!(
            button.action.permission.permission_type,
            AllowedPermission::SomeUser
        );
        assert_eq!(
            button.action.permission.specify_user_ids,
            vec!["u1".to_string()]
        );
        assert!(button.action.auto_send);
        assert_eq!(button.action.unsupport_tips.as_deref(), Some("不支持"));
    }

    #[test]
    fn rows_wrap_after_five_buttons() {
        let mut builder = Builder::new();
        for _ in 0..6 {
            builder = builder.command("c", "标签", "x", Vec::new());
        }
        let keyboard = builder.build().expect("keyboard");
        assert_eq!(keyboard.rows.len(), 2);
        assert_eq!(keyboard.rows[0].buttons.len(), 5);
        assert_eq!(keyboard.rows[1].buttons.len(), 1);
    }

    #[test]
    fn explicit_row_breaks_line() {
        let keyboard = Builder::new()
            .command("a", "A", "x", Vec::new())
            .row()
            .command("b", "B", "x", Vec::new())
            .build()
            .expect("keyboard");
        assert_eq!(keyboard.rows.len(), 2);
    }

    #[test]
    fn empty_build_is_none() {
        assert!(Builder::new().build().is_none());
    }

    #[test]
    fn empty_whitelist_allows_all() {
        let keyboard = Builder::new()
            .callback("confirm", "确认", "x", vec![only(Vec::new())])
            .build()
            .expect("keyboard");
        assert_eq!(
            keyboard.rows[0].buttons[0]
                .action
                .permission
                .permission_type,
            AllowedPermission::AllUser
        );
    }

    #[test]
    fn board_full_drops_extra_buttons() {
        let mut builder = Builder::new();
        for row in 0..MAX_ROWS {
            for _ in 0..MAX_BUTTONS_PER_ROW {
                builder = builder.command("c", "标签", "x", Vec::new());
            }
            if row + 1 < MAX_ROWS {
                builder = builder.row();
            }
        }
        builder = builder.command("overflow", "溢出", "x", Vec::new());
        let keyboard = builder.build().expect("keyboard");
        assert_eq!(keyboard.rows.len(), MAX_ROWS);
        assert!(!keyboard.has_button("overflow"));
    }
}
