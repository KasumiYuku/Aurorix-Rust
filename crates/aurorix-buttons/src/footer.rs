use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use aurorix_core::Scene;
use regex::Regex;

use crate::callbacks::CallbackRegistry;
use crate::keyboard::{
    AllowedPermission, Button, ButtonStyle, Keyboard, DEFAULT_UNSUPPORTED_TIP, MAX_ROWS,
};

pub const FOOTER_ID_PREFIX: &str = "aurorix.footer.";

pub const FOOTER_MAX_BUTTONS: i32 = 5;

pub const FOOTER_OPTION_SEP: char = ';';

pub const FOOTER_DEFAULT_REPLY: &str = "已记录，当前 {{count}} 次。";

pub const FOOTER_REPLY_PLACEHOLDERS: [&str; 4] = ["count", "user", "group", "label"];

static FOOTER_PLACEHOLDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{([^}]*)\}\}").expect("footer placeholder regex"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FooterScope {
    Global = 0,
    NoticeWelcome = 1,
    NoticeFarewell = 2,
    SceneGroup = 3,
    SceneC2c = 4,
    SceneChannel = 5,
    SceneDm = 6,
}

impl FooterScope {
    pub const ALL: [FooterScope; 7] = [
        FooterScope::Global,
        FooterScope::NoticeWelcome,
        FooterScope::NoticeFarewell,
        FooterScope::SceneGroup,
        FooterScope::SceneC2c,
        FooterScope::SceneChannel,
        FooterScope::SceneDm,
    ];

    pub const fn slot_base(self) -> i32 {
        self as i32 * FOOTER_MAX_BUTTONS
    }

    pub const fn slug(self) -> &'static str {
        match self {
            FooterScope::Global => "",
            FooterScope::NoticeWelcome => "notice.welcome",
            FooterScope::NoticeFarewell => "notice.farewell",
            FooterScope::SceneGroup => "scene.group",
            FooterScope::SceneC2c => "scene.c2c",
            FooterScope::SceneChannel => "scene.channel",
            FooterScope::SceneDm => "scene.dm",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            FooterScope::Global => "全局",
            FooterScope::NoticeWelcome => "入群欢迎",
            FooterScope::NoticeFarewell => "退群提示",
            FooterScope::SceneGroup => "群聊",
            FooterScope::SceneC2c => "单聊",
            FooterScope::SceneChannel => "频道",
            FooterScope::SceneDm => "频道私信",
        }
    }

    pub const fn from_scene(scene: Scene) -> Option<Self> {
        match scene {
            Scene::Group => Some(FooterScope::SceneGroup),
            Scene::C2c => Some(FooterScope::SceneC2c),
            Scene::Channel => Some(FooterScope::SceneChannel),
            Scene::Dm => Some(FooterScope::SceneDm),
        }
    }
}

pub fn footer_slot_of(slot: i32) -> Option<(FooterScope, i32)> {
    if slot < 0 {
        return None;
    }
    let index = slot / FOOTER_MAX_BUTTONS;
    if index as usize >= FooterScope::ALL.len() {
        return None;
    }
    Some((FooterScope::ALL[index as usize], slot % FOOTER_MAX_BUTTONS))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FooterKind {
    Counter,
    Command,
    Link,
    Repeat,
    Image,
    Callback,
}

impl FooterKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            FooterKind::Counter => "counter",
            FooterKind::Command => "command",
            FooterKind::Link => "link",
            FooterKind::Repeat => "repeat",
            FooterKind::Image => "image",
            FooterKind::Callback => "callback",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "counter" | "计数" | "点赞" => Ok(FooterKind::Counter),
            "command" | "指令" => Ok(FooterKind::Command),
            "link" | "链接" => Ok(FooterKind::Link),
            "repeat" | "复读" | "回填" => Ok(FooterKind::Repeat),
            "image" | "图片" | "图" => Ok(FooterKind::Image),
            "callback" | "回调" => Ok(FooterKind::Callback),
            other => Err(format!(
                "类型 {other:?} 不认识, 只能是 counter(计数) / command(指令) / link(链接) / repeat(复读) / image(图片) / callback(回调)"
            )),
        }
    }

    pub const fn allowed_options(self) -> &'static [&'static str] {
        match self {
            FooterKind::Counter => &["key", "dedup", "scope", "style", "visited", "dup"],
            FooterKind::Command => &["key", "auto", "style", "visited"],
            FooterKind::Link => &["key", "style", "visited"],
            FooterKind::Repeat => &["auto", "style", "visited"],
            FooterKind::Image => &["style", "visited"],
            FooterKind::Callback => &["style", "visited"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FooterSpec {
    pub label: String,
    pub kind: Option<FooterKind>,
    pub arg: String,
    pub options: BTreeMap<String, String>,
    pub key: String,
}

impl FooterSpec {
    pub fn option(&self, name: &str, fallback: &str) -> String {
        match self.options.get(name) {
            Some(value) if !value.is_empty() => value.clone(),
            _ => fallback.to_string(),
        }
    }

    pub fn kind(&self) -> FooterKind {
        self.kind.unwrap_or(FooterKind::Counter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FooterParseResult {
    pub specs: Vec<FooterSpec>,
    pub rows: Vec<Vec<usize>>,
    pub errors: Vec<String>,
}

impl FooterParseResult {
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    pub fn row_specs(&self, index: usize) -> Vec<&FooterSpec> {
        self.rows
            .get(index)
            .map(|row| {
                row.iter()
                    .filter_map(|slot| self.specs.get(*slot))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FooterDedup {
    None,
    Group,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FooterBucket {
    Group,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FooterAction {
    Counter {
        count_key: String,
        dedup: FooterDedup,
        bucket: FooterBucket,
        reply: String,
        duplicate_reply: Option<String>,
        label: String,
    },
    Image {
        url: String,
        label: String,
    },
    Command,
    Repeat,
    Link,
    Callback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterClick {
    pub scope: FooterScope,
    pub index: i32,
    pub slot: i32,
    pub label: String,
    pub action: FooterAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FooterClickError {
    BadData(String),
    UnknownSlot(i32),
    StaleConfig { scope: FooterScope, index: i32 },
}

impl std::fmt::Display for FooterClickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FooterClickError::BadData(message) => write!(f, "{message}"),
            FooterClickError::UnknownSlot(slot) => {
                write!(f, "底部按钮槽位 {slot} 不属于任何作用域")
            }
            FooterClickError::StaleConfig { scope, index } => {
                write!(
                    f,
                    "{}底部按钮 #{index} 已不在配置里(配置刚改过?), 忽略本次点击",
                    scope.label()
                )
            }
        }
    }
}

impl std::error::Error for FooterClickError {}

pub fn footer_dedup_of(spec: &FooterSpec) -> FooterDedup {
    match spec.option("dedup", "user").to_lowercase().as_str() {
        "none" => FooterDedup::None,
        "group" => FooterDedup::Group,
        _ => FooterDedup::User,
    }
}

pub fn footer_bucket_of(spec: &FooterSpec) -> FooterBucket {
    match spec.option("scope", "group").to_lowercase().as_str() {
        "global" => FooterBucket::Global,
        _ => FooterBucket::Group,
    }
}

pub fn footer_action_of(scope: FooterScope, spec: &FooterSpec) -> FooterAction {
    match spec.kind() {
        FooterKind::Counter => {
            let reply = if spec.arg.trim().is_empty() {
                FOOTER_DEFAULT_REPLY.to_string()
            } else {
                spec.arg.clone()
            };
            let duplicate = spec.option("dup", "");
            FooterAction::Counter {
                count_key: footer_count_key(scope, spec),
                dedup: footer_dedup_of(spec),
                bucket: footer_bucket_of(spec),
                reply,
                duplicate_reply: if duplicate.is_empty() {
                    None
                } else {
                    Some(duplicate)
                },
                label: spec.label.clone(),
            }
        }
        FooterKind::Image => FooterAction::Image {
            url: spec.arg.clone(),
            label: spec.label.clone(),
        },
        FooterKind::Command => FooterAction::Command,
        FooterKind::Repeat => FooterAction::Repeat,
        FooterKind::Link => FooterAction::Link,
        FooterKind::Callback => FooterAction::Callback,
    }
}

pub fn parse_footer(lines: &[String]) -> FooterParseResult {
    let mut out = FooterParseResult::default();
    let mut seen_keys: BTreeMap<String, usize> = BTreeMap::new();
    for (index, raw) in lines.iter().enumerate() {
        let line_no = index + 1;
        let mut row: Vec<usize> = Vec::new();
        for chunk in split_footer_line(raw) {
            let text = chunk.trim();
            if text.is_empty() || text.starts_with('#') {
                continue;
            }
            if out.specs.len() >= FOOTER_MAX_BUTTONS as usize {
                out.errors.push(format!(
                    "第 {line_no} 行: 底部按钮最多 {FOOTER_MAX_BUTTONS} 个, 多出的被忽略"
                ));
                continue;
            }
            match parse_footer_line(text) {
                Ok(spec) => {
                    if spec.kind() == FooterKind::Counter {
                        if let Some(first) = seen_keys.get(&spec.key).copied() {
                            out.errors.push(format!(
                                "第 {line_no} 行: 计数键 {:?} 与第 {first} 行重复, 用 key= 区分(或删掉一个), 该按钮被忽略",
                                spec.key
                            ));
                            continue;
                        }
                        seen_keys.insert(spec.key.clone(), line_no);
                    }
                    row.push(out.specs.len());
                    out.specs.push(spec);
                }
                Err(err) => out.errors.push(format!("第 {line_no} 行: {err}")),
            }
        }
        if !row.is_empty() {
            out.rows.push(row);
        }
    }
    out
}

pub fn split_footer_line(line: &str) -> Vec<String> {
    if !line.contains(',') {
        return vec![line.to_string()];
    }
    let mut chunks = line.split(',');
    let mut out = vec![chunks.next().unwrap_or("").to_string()];
    for chunk in chunks {
        if footer_looks_like_button(chunk) {
            out.push(chunk.to_string());
            continue;
        }
        let last = out.len() - 1;
        out[last].push(',');
        out[last].push_str(chunk);
    }
    out
}

fn footer_looks_like_button(chunk: &str) -> bool {
    let parts: Vec<&str> = chunk.split('|').collect();
    if parts.len() < 2 {
        return false;
    }
    FooterKind::parse(parts[1].trim()).is_ok()
}

pub fn parse_footer_line(text: &str) -> Result<FooterSpec, String> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() < 2 {
        return Err("至少要写「标签 | 类型」两段".to_string());
    }
    if parts.len() > 4 {
        return Err(format!(
            "最多 4 段「标签 | 类型 | 参数 | 选项」, 现在有 {} 段(文案里不要出现 | 符号)",
            parts.len()
        ));
    }
    let parts: Vec<String> = parts.iter().map(|part| part.trim().to_string()).collect();
    if parts[0].is_empty() {
        return Err("标签不能为空".to_string());
    }
    let kind = FooterKind::parse(&parts[1])?;
    let mut spec = FooterSpec {
        label: parts[0].clone(),
        kind: Some(kind),
        ..FooterSpec::default()
    };
    let mut option_text = String::new();
    match parts.len() {
        4 => {
            spec.arg = parts[2].clone();
            option_text = parts[3].clone();
        }
        3 => {
            let (arg, options) = split_inline_footer_options(&parts[2]);
            spec.arg = arg;
            option_text = options;
        }
        _ => {}
    }
    spec.options = parse_footer_options(&option_text)?;
    spec.key = spec.option("key", &spec.label);
    validate_spec(&spec)?;
    Ok(spec)
}

pub fn parse_footer_options(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut options = BTreeMap::new();
    for pair in text.split(FOOTER_OPTION_SEP) {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let Some((name, value)) = pair.split_once('=') else {
            return Err(format!("选项 {pair:?} 不是 k=v 形式, 例如 dedup=user"));
        };
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        if name.is_empty() {
            return Err(format!("选项 {pair:?} 缺少名字"));
        }
        options.insert(name, value);
    }
    Ok(options)
}

pub fn split_inline_footer_options(raw: &str) -> (String, String) {
    let chunks: Vec<&str> = raw.split(FOOTER_OPTION_SEP).collect();
    if chunks.len() < 2 {
        return (raw.to_string(), String::new());
    }
    for chunk in &chunks[1..] {
        let trimmed = chunk.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.split_once('=') {
            Some((name, _)) if !name.trim().is_empty() => {}
            _ => return (raw.to_string(), String::new()),
        }
    }
    (chunks[0].trim().to_string(), chunks[1..].join(";"))
}

fn validate_spec(spec: &FooterSpec) -> Result<(), String> {
    let kind = spec.kind();
    for name in spec.options.keys() {
        if !kind.allowed_options().contains(&name.as_str()) {
            return Err(format!(
                "类型 {} 不支持选项 {name:?}(可用: {})",
                kind.as_str(),
                kind.allowed_options().join(", ")
            ));
        }
    }
    if let Some(style) = spec.options.get("style") {
        if !is_valid_style_option(style) {
            return Err(format!("style 只能是 blue / gray / 0~4, 收到 {style:?}"));
        }
    }
    match kind {
        FooterKind::Counter => {
            if let Some(scope) = spec.options.get("scope") {
                if scope != "group" && scope != "global" {
                    return Err(format!("scope 只能是 group 或 global, 收到 {scope:?}"));
                }
            }
            if let Some(dedup) = spec.options.get("dedup") {
                if dedup != "user" && dedup != "group" && dedup != "none" {
                    return Err(format!("dedup 只能是 user / group / none, 收到 {dedup:?}"));
                }
            }
            check_footer_placeholders("回复模板", &spec.arg)?;
            if let Some(dup) = spec.options.get("dup") {
                check_footer_placeholders("dup 文案", dup)?;
            }
        }
        FooterKind::Command => {
            if spec.arg.is_empty() {
                return Err("command 要在第 3 段写指令内容, 例如 /help".to_string());
            }
            if let Some(auto) = spec.options.get("auto") {
                if auto != "0" && auto != "1" {
                    return Err(format!("auto 只能是 0 或 1, 收到 {auto:?}"));
                }
            }
        }
        FooterKind::Link => {
            if !is_http_url(&spec.arg) {
                return Err(format!(
                    "link 的第 3 段要是 http(s) 链接, 收到 {:?}",
                    spec.arg
                ));
            }
        }
        FooterKind::Repeat => {
            if !spec.arg.is_empty() {
                return Err(
                    "repeat(复读) 不用写第 3 段: 它填的就是触发这条回复的原消息, 把这段删掉"
                        .to_string(),
                );
            }
            if let Some(auto) = spec.options.get("auto") {
                if auto != "0" && auto != "1" {
                    return Err(format!("auto 只能是 0 或 1, 收到 {auto:?}"));
                }
            }
        }
        FooterKind::Image => {
            if !is_http_url(&spec.arg) {
                return Err(format!(
                    "image 的第 3 段要是 http(s) 图片链接, 收到 {:?}",
                    spec.arg
                ));
            }
        }
        FooterKind::Callback => {
            if spec.arg.is_empty() {
                return Err(
                    "callback(回调) 要在第 3 段写回调 id, 例如 okamikko.noat.guide".to_string(),
                );
            }
            if !CallbackRegistry::global().has(&spec.arg) {
                return Err(format!(
                    "callback(回调) 指向的 {:?} 没有注册, 点了不会有反应",
                    spec.arg
                ));
            }
        }
    }
    Ok(())
}

pub fn check_footer_placeholders(where_: &str, template: &str) -> Result<(), String> {
    for capture in FOOTER_PLACEHOLDER_RE.captures_iter(template) {
        let name = capture[1].trim();
        if !FOOTER_REPLY_PLACEHOLDERS.contains(&name) {
            return Err(format!(
                "{where_} 里的占位符 {:?} 不认识(可用: {{{{count}}}} {{{{user}}}} {{{{group}}}} {{{{label}}}})",
                &capture[0]
            ));
        }
    }
    Ok(())
}

pub fn is_valid_style_option(value: &str) -> bool {
    matches!(value, "blue" | "gray" | "0" | "1" | "2" | "3" | "4")
}

pub fn is_http_url(value: &str) -> bool {
    let Some(rest) = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
    else {
        return false;
    };
    if value.contains(char::is_whitespace) {
        return false;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty() && !host.starts_with(':')
}

pub fn footer_button_id(index: i32) -> String {
    format!("{FOOTER_ID_PREFIX}{index}")
}

pub fn footer_default_visited(kind: FooterKind) -> &'static str {
    match kind {
        FooterKind::Counter => "已投",
        FooterKind::Command | FooterKind::Repeat => "已填入",
        FooterKind::Image | FooterKind::Callback => "已发送",
        FooterKind::Link => "",
    }
}

pub fn footer_style(name: &str) -> ButtonStyle {
    match name.trim().to_lowercase().as_str() {
        "0" | "gray" | "grey" => ButtonStyle::Gray,
        "1" | "blue" => ButtonStyle::Blue,
        "2" => ButtonStyle::GraySolid,
        "3" => ButtonStyle::BlueSolid,
        _ => ButtonStyle::BlueText,
    }
}

pub fn footer_button(spec: &FooterSpec, index: i32, original_input: &str) -> Option<Button> {
    let kind = spec.kind();
    let mut button = Button {
        id: footer_button_id(index),
        render_data: crate::RenderData {
            label: spec.label.clone(),
            visited_label: spec.option("visited", footer_default_visited(kind)),
            style: footer_style(&spec.option("style", "")),
        },
        action: crate::Action::default(),
    };
    match kind {
        FooterKind::Counter => {
            button.set_callback(&index.to_string());
        }
        FooterKind::Command => {
            button.set_auto_command(&spec.arg, spec.option("auto", "0") == "1", false);
        }
        FooterKind::Link => {
            button.set_href(&spec.arg);
        }
        FooterKind::Repeat => {
            if original_input.trim().is_empty() {
                return None;
            }
            button.set_auto_command(original_input, spec.option("auto", "0") == "1", false);
        }
        FooterKind::Image => {
            button.set_callback(&index.to_string());
        }
        FooterKind::Callback => {
            button.id = spec.arg.clone();
            button.set_callback(&spec.arg);
        }
    }
    button.set_permission(AllowedPermission::AllUser);
    button.set_unsupported_tip(DEFAULT_UNSUPPORTED_TIP);
    Some(button)
}

pub fn footer_buttons(specs: &[FooterSpec], original_input: &str) -> Vec<Button> {
    specs
        .iter()
        .enumerate()
        .filter_map(|(index, spec)| footer_button(spec, index as i32, original_input))
        .collect()
}

pub fn footer_button_rows(
    result: &FooterParseResult,
    base: i32,
    original_input: &str,
) -> Vec<Vec<Button>> {
    let mut out = Vec::new();
    let mut flat = 0usize;
    for row in &result.rows {
        let mut buttons = Vec::with_capacity(row.len());
        for _ in row {
            if let Some(spec) = result.specs.get(flat) {
                if let Some(button) = footer_button(spec, base + flat as i32, original_input) {
                    buttons.push(button);
                }
            }
            flat += 1;
        }
        if !buttons.is_empty() {
            out.push(buttons);
        }
    }
    out
}

type FooterSource = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

#[derive(Debug, Clone, Default)]
struct FooterCache {
    ready: bool,
    raw: String,
    result: Option<FooterParseResult>,
}

#[derive(Default)]
pub struct FooterHub {
    sources: Mutex<BTreeMap<FooterScope, FooterSource>>,
    caches: Mutex<BTreeMap<FooterScope, FooterCache>>,
}

impl std::fmt::Debug for FooterHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FooterHub")
            .field("scopes", &self.lock_sources().keys().collect::<Vec<_>>())
            .finish()
    }
}

impl FooterHub {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_sources(&self) -> MutexGuard<'_, BTreeMap<FooterScope, FooterSource>> {
        self.sources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_caches(&self) -> MutexGuard<'_, BTreeMap<FooterScope, FooterCache>> {
        self.caches
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_source(&self, scope: FooterScope, source: Option<FooterSource>) {
        {
            let mut sources = self.lock_sources();
            match source {
                Some(source) => {
                    sources.insert(scope, source);
                }
                None => {
                    sources.remove(&scope);
                }
            }
        }
        self.lock_caches().remove(&scope);
    }

    pub fn set_lines(&self, scope: FooterScope, lines: Vec<String>) {
        self.set_source(scope, Some(Arc::new(move || lines.clone())));
    }

    pub fn is_enabled(&self, scope: FooterScope) -> bool {
        self.lock_sources().contains_key(&scope)
    }

    pub fn current_result(&self, scope: FooterScope) -> (FooterParseResult, Vec<String>) {
        let sources = self.lock_sources();
        let Some(source) = sources.get(&scope).cloned() else {
            return (FooterParseResult::default(), Vec::new());
        };
        drop(sources);
        let raw = source().join("\n");
        let mut caches = self.lock_caches();
        let cache = caches.entry(scope).or_default();
        if cache.ready && cache.raw == raw {
            return match &cache.result {
                Some(result) => (result.clone(), Vec::new()),
                None => (FooterParseResult::default(), Vec::new()),
            };
        }
        let result = parse_footer(&raw.split('\n').map(str::to_string).collect::<Vec<_>>());
        cache.ready = true;
        cache.raw = raw;
        cache.result = Some(result.clone());
        let errors = result.errors.clone();
        (result, errors)
    }

    pub fn merge(
        &self,
        board: Option<Keyboard>,
        original_input: &str,
        scene: Scene,
    ) -> Option<Keyboard> {
        let mut scope = FooterScope::Global;
        if let Some(scene_scope) = FooterScope::from_scene(scene) {
            let (result, _) = self.current_result(scene_scope);
            if !result.specs.is_empty() {
                scope = scene_scope;
            }
        }
        let (result, _) = self.current_result(scope);
        if result.specs.is_empty() {
            return board;
        }
        let rows = footer_button_rows(&result, scope.slot_base(), original_input);
        if rows.is_empty() {
            return board;
        }
        match board {
            None => {
                let mut fresh = Keyboard::new();
                if !append_footer_rows(&mut fresh, &rows) {
                    return None;
                }
                Some(fresh)
            }
            Some(mut keyboard) => {
                append_footer_rows(&mut keyboard, &rows);
                Some(keyboard)
            }
        }
    }

    pub fn keyboard(&self, scope: FooterScope, original_input: &str) -> Option<Keyboard> {
        let (result, _) = self.current_result(scope);
        let rows = footer_button_rows(&result, scope.slot_base(), original_input);
        if rows.is_empty() {
            return None;
        }
        let mut keyboard = Keyboard::new();
        if !append_footer_rows(&mut keyboard, &rows) {
            return None;
        }
        Some(keyboard)
    }

    pub fn resolve_click(&self, data: &str) -> Result<FooterClick, FooterClickError> {
        let trimmed = data.trim();
        let slot = trimmed
            .parse::<i32>()
            .map_err(|_| FooterClickError::BadData(format!("底部按钮回调数据非法: {data:?}")))?;
        let (scope, index) = footer_slot_of(slot).ok_or(FooterClickError::UnknownSlot(slot))?;
        let (result, _) = self.current_result(scope);
        let Some(spec) = result.specs.get(index as usize) else {
            return Err(FooterClickError::StaleConfig { scope, index });
        };
        Ok(FooterClick {
            scope,
            index,
            slot,
            label: spec.label.clone(),
            action: footer_action_of(scope, spec),
        })
    }
}

fn append_footer_rows(keyboard: &mut Keyboard, rows: &[Vec<Button>]) -> bool {
    if keyboard.remaining_rows() < rows.len() {
        return false;
    }
    for row in rows {
        if !keyboard.append_row(row.clone()) {
            return false;
        }
    }
    true
}

pub fn footer_count_key(scope: FooterScope, spec: &FooterSpec) -> String {
    match scope.slug() {
        "" => format!("footer.{}", spec.key),
        slug => format!("footer.{slug}.{}", spec.key),
    }
}

pub fn footer_button_id_count() -> i32 {
    FooterScope::ALL.len() as i32 * FOOTER_MAX_BUTTONS
}

pub fn footer_max_rows() -> usize {
    MAX_ROWS
}
