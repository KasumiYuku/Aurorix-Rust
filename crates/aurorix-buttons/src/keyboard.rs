use std::sync::Arc;
use std::sync::LazyLock;

use arc_swap::ArcSwap;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::error::{ButtonError, ButtonResult};

pub const DEFAULT_UNSUPPORTED_TIP: &str = "当前客户端不支持按钮";

pub const MAX_ROWS: usize = 5;

pub const MAX_BUTTONS_PER_ROW: usize = 5;

pub const FONT_SIZE_SMALL: &str = "small";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum AllowedPermission {
    #[default]
    SomeUser = 0,
    Admin = 1,
    AllUser = 2,
}

impl AllowedPermission {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(AllowedPermission::SomeUser),
            1 => Some(AllowedPermission::Admin),
            2 => Some(AllowedPermission::AllUser),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum ButtonAction {
    #[default]
    Link = 0,
    Callback = 1,
    Command = 2,
}

impl ButtonAction {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(ButtonAction::Link),
            1 => Some(ButtonAction::Callback),
            2 => Some(ButtonAction::Command),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum ButtonStyle {
    #[default]
    Gray = 0,
    Blue = 1,
    GraySolid = 2,
    BlueSolid = 3,
    BlueText = 4,
}

impl ButtonStyle {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(ButtonStyle::Gray),
            1 => Some(ButtonStyle::Blue),
            2 => Some(ButtonStyle::GraySolid),
            3 => Some(ButtonStyle::BlueSolid),
            4 => Some(ButtonStyle::BlueText),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenderData {
    pub label: String,
    pub visited_label: String,
    pub style: ButtonStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    pub permission_type: AllowedPermission,
    pub specify_user_ids: Vec<String>,
}

impl Default for Permission {
    fn default() -> Self {
        Self {
            permission_type: AllowedPermission::AllUser,
            specify_user_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Action {
    pub action_type: ButtonAction,
    pub url: Option<String>,
    pub callback_data: Option<String>,
    pub msg: Option<String>,
    pub reply: bool,
    pub auto_send: bool,
    pub anchor: bool,
    pub permission: Permission,
    pub unsupport_tips: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Button {
    pub id: String,
    pub render_data: RenderData,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ButtonRow {
    pub buttons: Vec<Button>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyboardStyle {
    pub font_size: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Keyboard {
    pub style: Option<KeyboardStyle>,
    pub rows: Vec<ButtonRow>,
    pub font_size_set: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandNormalizerState {
    Unset,
    Installed,
}

type TextHook = Arc<dyn Fn(&str) -> String + Send + Sync>;

static COMMAND_NORMALIZER: LazyLock<ArcSwap<Option<TextHook>>> =
    LazyLock::new(|| ArcSwap::from_pointee(None));

type FontSizeOverride = ArcSwap<Option<Arc<dyn Fn() -> String + Send + Sync>>>;

static DEFAULT_FONT_SIZE: LazyLock<FontSizeOverride> =
    LazyLock::new(|| ArcSwap::from_pointee(None));

pub fn set_command_normalizer(hook: Option<TextHook>) -> CommandNormalizerState {
    let installed = hook.is_some();
    COMMAND_NORMALIZER.store(Arc::new(hook));
    if installed {
        CommandNormalizerState::Installed
    } else {
        CommandNormalizerState::Unset
    }
}

pub fn set_default_font_size(source: Option<Arc<dyn Fn() -> String + Send + Sync>>) {
    DEFAULT_FONT_SIZE.store(Arc::new(source));
}

pub fn normalize_font_size(size: &str) -> String {
    size.trim().to_lowercase()
}

fn current_default_font_size() -> String {
    let hook = DEFAULT_FONT_SIZE.load_full();
    match hook.as_ref() {
        Some(source) => normalize_font_size(&source()),
        None => String::new(),
    }
}

fn apply_command_normalizer(msg: String) -> String {
    let hook = COMMAND_NORMALIZER.load_full();
    match hook.as_ref() {
        Some(normalizer) => normalizer(&msg),
        None => msg,
    }
}

impl Keyboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn effective_style(&self) -> Option<KeyboardStyle> {
        if self.font_size_set {
            return self.style.clone();
        }
        let size = current_default_font_size();
        if size.is_empty() {
            return self.style.clone();
        }
        Some(KeyboardStyle {
            font_size: Some(size),
        })
    }

    pub fn set_font_size(&mut self, size: &str) -> &mut Self {
        self.font_size_set = true;
        let size = normalize_font_size(size);
        if size.is_empty() {
            self.style = None;
            return self;
        }
        match self.style.as_mut() {
            Some(style) => style.font_size = Some(size),
            None => {
                self.style = Some(KeyboardStyle {
                    font_size: Some(size),
                })
            }
        }
        self
    }

    pub fn append_button(
        &mut self,
        id: &str,
        label: &str,
        visited: &str,
        style: ButtonStyle,
        row: i32,
    ) -> ButtonResult<&mut Button> {
        if !(0..MAX_ROWS as i32).contains(&row) {
            return Err(ButtonError::RowOutOfRange(row));
        }
        let row = row as usize;
        while self.rows.len() <= row {
            self.rows.push(ButtonRow::default());
        }
        if self.rows[row].buttons.len() == MAX_BUTTONS_PER_ROW {
            return Err(ButtonError::RowFull(row as i32));
        }
        self.rows[row].buttons.push(Button {
            id: id.to_string(),
            render_data: RenderData {
                label: label.to_string(),
                visited_label: visited.to_string(),
                style,
            },
            action: Action::default(),
        });
        Ok(self.rows[row]
            .buttons
            .last_mut()
            .expect("just pushed button"))
    }

    pub fn append_row(&mut self, buttons: Vec<Button>) -> bool {
        if buttons.is_empty() || self.rows.len() >= MAX_ROWS {
            return false;
        }
        self.rows.push(ButtonRow { buttons });
        true
    }
    pub fn remaining_rows(&self) -> usize {
        MAX_ROWS.saturating_sub(self.rows.len())
    }

    pub fn flatten(&self) -> Vec<Button> {
        self.rows
            .iter()
            .flat_map(|row| row.buttons.iter().cloned())
            .collect()
    }

    pub fn has_button(&self, id: &str) -> bool {
        self.rows
            .iter()
            .any(|row| row.buttons.iter().any(|button| button.id == id))
    }

    pub fn validate(&self) -> ButtonResult<()> {
        if self.rows.len() > MAX_ROWS {
            return Err(ButtonError::TooManyRows(self.rows.len()));
        }
        for (index, row) in self.rows.iter().enumerate() {
            if row.buttons.len() > MAX_BUTTONS_PER_ROW {
                return Err(ButtonError::TooManyButtons {
                    row: index,
                    count: row.buttons.len(),
                });
            }
            for button in &row.buttons {
                button.validate_action()?;
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> ButtonResult<Vec<u8>> {
        if self.rows.is_empty() {
            return Ok(Vec::new());
        }
        if self.rows.len() > MAX_ROWS {
            return Err(ButtonError::TooManyRows(self.rows.len()));
        }
        for (index, row) in self.rows.iter().enumerate() {
            if row.buttons.len() > MAX_BUTTONS_PER_ROW {
                return Err(ButtonError::TooManyButtons {
                    row: index,
                    count: row.buttons.len(),
                });
            }
        }
        self.validate()?;
        serde_json::to_vec(self).map_err(|err| ButtonError::Json(err.to_string()))
    }

    pub fn merge_trailing(&mut self, per_row: usize, extra: Vec<Button>) -> &mut Self {
        let origin = self.flatten();
        if origin.is_empty() {
            if !extra.is_empty() {
                self.rows = vec![ButtonRow { buttons: extra }];
            }
            return self;
        }
        let per_row = if per_row < 1 {
            MAX_BUTTONS_PER_ROW
        } else {
            per_row
        };
        let mut all = origin;
        all.extend(extra);
        let mut rows: Vec<ButtonRow> = Vec::new();
        rows.push(ButtonRow {
            buttons: vec![all[0].clone()],
        });
        let mut rest = &all[1..];
        while !rest.is_empty() && rows.len() < MAX_ROWS {
            let take = per_row.min(rest.len());
            rows.push(ButtonRow {
                buttons: rest[..take].to_vec(),
            });
            rest = &rest[take..];
        }
        self.rows = rows;
        self
    }
}

impl Action {
    pub fn action_data(&self) -> ButtonResult<String> {
        match self.action_type {
            ButtonAction::Callback => self
                .callback_data
                .clone()
                .ok_or_else(|| ButtonError::CallbackWithoutData(String::new())),
            ButtonAction::Command => self
                .msg
                .clone()
                .map(apply_command_normalizer)
                .ok_or_else(|| ButtonError::CommandWithoutMsg(String::new())),
            ButtonAction::Link => self
                .url
                .clone()
                .ok_or_else(|| ButtonError::LinkWithoutUrl(String::new())),
        }
    }
}

impl Button {
    pub fn set_auto_command(&mut self, content: &str, auto_send: bool, anchor: bool) -> &mut Self {
        self.action.anchor = anchor;
        self.action.msg = Some(content.to_string());
        self.action.action_type = ButtonAction::Command;
        self.action.auto_send = auto_send;
        self
    }

    pub fn set_href(&mut self, url: &str) -> &mut Self {
        self.action.action_type = ButtonAction::Link;
        self.action.url = Some(url.to_string());
        self
    }

    pub fn set_callback(&mut self, data: &str) -> &mut Self {
        self.action.callback_data = Some(data.to_string());
        self.action.action_type = ButtonAction::Callback;
        self
    }

    pub fn set_permission(&mut self, required: AllowedPermission) -> &mut Self {
        self.action.permission.permission_type = required;
        self
    }

    pub fn set_user_white_list(&mut self, users: &[String]) -> &mut Self {
        if users.is_empty() {
            self.action.permission.permission_type = AllowedPermission::AllUser;
            self.action.permission.specify_user_ids = Vec::new();
            return self;
        }
        self.action.permission.permission_type = AllowedPermission::SomeUser;
        self.action.permission.specify_user_ids = users.to_vec();
        self
    }

    pub fn set_unsupported_tip(&mut self, tip: &str) -> &mut Self {
        self.action.unsupport_tips = Some(tip.to_string());
        self
    }

    fn validate_action(&self) -> ButtonResult<()> {
        let id = || self.id.clone();
        match self.action.action_type {
            ButtonAction::Callback => {
                if self
                    .action
                    .callback_data
                    .as_deref()
                    .unwrap_or("")
                    .is_empty()
                {
                    return Err(ButtonError::CallbackWithoutData(id()));
                }
            }
            ButtonAction::Command => {
                if self.action.msg.as_deref().unwrap_or("").is_empty() {
                    return Err(ButtonError::CommandWithoutMsg(id()));
                }
            }
            ButtonAction::Link => {
                if self.action.url.as_deref().unwrap_or("").is_empty() {
                    return Err(ButtonError::LinkWithoutUrl(id()));
                }
            }
        }
        Ok(())
    }

    pub fn action_json(&self) -> ButtonResult<ActionJson> {
        self.validate_action()?;
        Ok(ActionJson {
            action_type: self.action.action_type.code(),
            data: self.action.action_data()?,
            enter: self.action.auto_send,
            reply: self.action.reply,
            anchor: self.action.anchor,
            unsupport_tips: self
                .action
                .unsupport_tips
                .clone()
                .filter(|tip| !tip.is_empty()),
            permission_type: self.action.permission.permission_type.code(),
            specify_user_ids: self.action.permission.specify_user_ids.clone(),
        })
    }
}

pub struct ActionJson {
    pub action_type: u8,
    pub data: String,
    pub enter: bool,
    pub reply: bool,
    pub anchor: bool,
    pub unsupport_tips: Option<String>,
    pub permission_type: u8,
    pub specify_user_ids: Vec<String>,
}

struct KeyboardContent<'a> {
    keyboard: &'a Keyboard,
}

impl Serialize for ActionJson {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("action", 7)?;
        state.serialize_field("type", &self.action_type)?;
        state.serialize_field("data", &self.data)?;
        if self.enter {
            state.serialize_field("enter", &true)?;
        }
        if self.reply {
            state.serialize_field("reply", &true)?;
        }
        if self.anchor {
            state.serialize_field("anchor", &true)?;
        }
        if let Some(tip) = &self.unsupport_tips {
            state.serialize_field("unsupport_tips", tip)?;
        }
        state.serialize_field(
            "permission",
            &PermissionJson {
                permission_type: self.permission_type,
                specify_user_ids: &self.specify_user_ids,
            },
        )?;
        state.end()
    }
}

struct PermissionJson<'a> {
    permission_type: u8,
    specify_user_ids: &'a [String],
}

impl Serialize for PermissionJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("permission", 2)?;
        state.serialize_field("type", &self.permission_type)?;
        if !self.specify_user_ids.is_empty() {
            state.serialize_field("specify_user_ids", self.specify_user_ids)?;
        }
        state.end()
    }
}

impl Serialize for Button {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error as _;
        let action = self
            .action_json()
            .map_err(|err| S::Error::custom(err.to_string()))?;
        let mut state = serializer.serialize_struct("button", 3)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("render_data", &self.render_data)?;
        state.serialize_field("action", &action)?;
        state.end()
    }
}

impl Serialize for RenderData {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("render_data", 3)?;
        state.serialize_field("label", &self.label)?;
        state.serialize_field("visited_label", &self.visited_label)?;
        state.serialize_field("style", &self.render_data_style())?;
        state.end()
    }
}

impl RenderData {
    fn render_data_style(&self) -> u8 {
        self.style.code()
    }
}

impl Serialize for ButtonRow {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("row", 1)?;
        state.serialize_field("buttons", &self.buttons)?;
        state.end()
    }
}

impl Serialize for Keyboard {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("keyboard", 1)?;
        state.serialize_field("content", &KeyboardContent { keyboard: self })?;
        state.end()
    }
}

impl Serialize for KeyboardContent<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let style = self.keyboard.effective_style();
        let mut state = serializer.serialize_struct("content", 2)?;
        if let Some(style) = style {
            state.serialize_field("style", &StyleJson { style: &style })?;
        }
        state.serialize_field("rows", &self.keyboard.rows)?;
        state.end()
    }
}

struct StyleJson<'a> {
    style: &'a KeyboardStyle,
}

impl Serialize for StyleJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("style", 1)?;
        if let Some(size) = &self.style.font_size {
            state.serialize_field("font_size", size)?;
        }
        state.end()
    }
}

pub fn qualify(scope: &str, id: &str) -> String {
    if scope.is_empty() {
        id.to_string()
    } else {
        format!("{scope}.{id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn hook_guard() -> std::sync::MutexGuard<'static, ()> {
        HOOK_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn plain_keyboard() -> Keyboard {
        crate::Builder::new()
            .command("info", "作者信息", "作者信息", Vec::new())
            .build()
            .expect("keyboard")
    }

    fn content_json(keyboard: &Keyboard) -> serde_json::Value {
        let raw = keyboard.to_json().expect("json");
        let parsed: serde_json::Value = serde_json::from_slice(&raw).expect("parse");
        parsed["content"].clone()
    }

    fn content_font_size(keyboard: &Keyboard) -> Option<String> {
        content_json(keyboard)
            .get("style")
            .and_then(|style| style.get("font_size"))
            .and_then(|size| size.as_str())
            .map(str::to_string)
    }

    #[test]
    fn font_size_lands_under_content_style() {
        let mut keyboard = plain_keyboard();
        keyboard.set_font_size(FONT_SIZE_SMALL);
        assert_eq!(content_font_size(&keyboard).as_deref(), Some("small"));
    }

    #[test]
    fn default_font_size_fills_unset_keyboard() {
        let _guard = hook_guard();
        set_default_font_size(Some(Arc::new(|| " SMALL ".to_string())));
        let size = content_font_size(&plain_keyboard());
        set_default_font_size(None);
        assert_eq!(size.as_deref(), Some("small"));
    }

    #[test]
    fn default_font_size_follows_source() {
        let _guard = hook_guard();
        let cell = Arc::new(std::sync::Mutex::new(String::new()));
        let source = {
            let cell = cell.clone();
            Arc::new(move || cell.lock().unwrap_or_else(|err| err.into_inner()).clone())
        };
        set_default_font_size(Some(source));
        let keyboard = plain_keyboard();
        let first = content_font_size(&keyboard);
        *cell.lock().unwrap_or_else(|err| err.into_inner()) = "small".to_string();
        let second = content_font_size(&keyboard);
        set_default_font_size(None);
        assert_eq!(first, None);
        assert_eq!(second.as_deref(), Some("small"));
    }

    #[test]
    fn explicit_empty_font_size_beats_default() {
        let _guard = hook_guard();
        set_default_font_size(Some(Arc::new(|| FONT_SIZE_SMALL.to_string())));
        let mut keyboard = plain_keyboard();
        keyboard.set_font_size("");
        let style = content_json(&keyboard).get("style").cloned();
        set_default_font_size(None);
        assert_eq!(style, None);
    }

    #[test]
    fn no_style_key_without_font_size() {
        let _guard = hook_guard();
        set_default_font_size(None);
        assert!(content_json(&plain_keyboard()).get("style").is_none());
    }

    #[test]
    fn builder_font_size_keeps_render_data_style() {
        let keyboard = crate::Builder::new()
            .small_font()
            .command("info", "作者信息", "作者信息", Vec::new())
            .build()
            .expect("keyboard");
        assert_eq!(content_font_size(&keyboard).as_deref(), Some("small"));
        let content = content_json(&keyboard);
        let style = content["rows"][0]["buttons"][0]["render_data"]["style"]
            .as_u64()
            .expect("numeric style");
        assert_eq!(style, u64::from(ButtonStyle::Gray.code()));
    }

    #[test]
    fn empty_keyboard_serializes_to_empty_bytes() {
        let keyboard = Keyboard::new();
        assert!(keyboard.to_json().expect("json").is_empty());
    }

    #[test]
    fn validation_rejects_legal_but_wrong_shapes() {
        let mut keyboard = Keyboard::new();
        for _ in 0..6 {
            keyboard.rows.push(ButtonRow::default());
        }
        assert_eq!(keyboard.to_json().unwrap_err(), ButtonError::TooManyRows(6));

        let mut wide = Keyboard::new();
        wide.rows.push(ButtonRow {
            buttons: vec![Button::default(); 6],
        });
        assert_eq!(
            wide.to_json().unwrap_err(),
            ButtonError::TooManyButtons { row: 0, count: 6 }
        );
    }

    #[test]
    fn validation_requires_payload_for_each_action() {
        let mut keyboard = Keyboard::new();
        keyboard.rows.push(ButtonRow {
            buttons: vec![Button {
                id: "cb".to_string(),
                action: Action {
                    action_type: ButtonAction::Callback,
                    ..Action::default()
                },
                ..Button::default()
            }],
        });
        assert_eq!(
            keyboard.to_json().unwrap_err(),
            ButtonError::CallbackWithoutData("cb".to_string())
        );

        let mut command = Keyboard::new();
        command.rows.push(ButtonRow {
            buttons: vec![Button {
                id: "cmd".to_string(),
                action: Action {
                    action_type: ButtonAction::Command,
                    ..Action::default()
                },
                ..Button::default()
            }],
        });
        assert_eq!(
            command.to_json().unwrap_err(),
            ButtonError::CommandWithoutMsg("cmd".to_string())
        );

        let mut link = Keyboard::new();
        link.rows.push(ButtonRow {
            buttons: vec![Button {
                id: "lnk".to_string(),
                action: Action {
                    action_type: ButtonAction::Link,
                    ..Action::default()
                },
                ..Button::default()
            }],
        });
        assert_eq!(
            link.to_json().unwrap_err(),
            ButtonError::LinkWithoutUrl("lnk".to_string())
        );
    }

    #[test]
    fn command_normalizer_rewrites_command_data() {
        let _guard = hook_guard();
        set_command_normalizer(Some(Arc::new(|msg: &str| msg.replace(" /", " "))));
        let raw = crate::Builder::new()
            .command("c", "标签", " /help", Vec::new())
            .build()
            .expect("keyboard")
            .to_json()
            .expect("json");
        set_command_normalizer(None);
        let text = String::from_utf8(raw).expect("utf8");
        assert!(text.contains("\"data\":\" help\""));
    }

    #[test]
    fn append_button_row_limits_match_go() {
        let mut keyboard = Keyboard::new();
        keyboard
            .append_button("a", "A", "A", ButtonStyle::Gray, 0)
            .expect("button");
        assert_eq!(
            keyboard
                .append_button("b", "B", "B", ButtonStyle::Gray, 5)
                .unwrap_err(),
            ButtonError::RowOutOfRange(5)
        );
        for index in 1..5 {
            keyboard
                .append_button(&format!("b{index}"), "B", "B", ButtonStyle::Gray, 0)
                .expect("button");
        }
        assert_eq!(
            keyboard
                .append_button("full", "F", "F", ButtonStyle::Gray, 0)
                .unwrap_err(),
            ButtonError::RowFull(0)
        );
    }

    #[test]
    fn merge_trailing_rebuilds_rows() {
        let mut keyboard = Keyboard::new();
        keyboard
            .append_button("a", "A", "A", ButtonStyle::Gray, 0)
            .expect("button");
        let extra = vec![Button {
            id: "x".to_string(),
            ..Button::default()
        }];
        keyboard.merge_trailing(2, extra);
        assert_eq!(keyboard.rows.len(), 2);
        assert_eq!(keyboard.rows[0].buttons.len(), 1);
        assert_eq!(keyboard.rows[1].buttons.len(), 1);

        let mut empty = Keyboard::new();
        empty.merge_trailing(0, vec![Button::default()]);
        assert_eq!(empty.rows.len(), 1);
    }
}
