use aurorix_core::{RoleRequired, Scene, Target};
use aurorix_gateway::ProtocolEvent;
use aurorix_plugin::{Incoming, IncomingAttachment};
use serde_json::Value;

pub const GROUP_AT_MESSAGE_CREATE: &str = "GROUP_AT_MESSAGE_CREATE";

pub const GROUP_MESSAGE_CREATE: &str = "GROUP_MESSAGE_CREATE";

pub const C2C_MESSAGE_CREATE: &str = "C2C_MESSAGE_CREATE";

pub const AT_MESSAGE_CREATE: &str = "AT_MESSAGE_CREATE";

pub const MESSAGE_CREATE: &str = "MESSAGE_CREATE";

pub const DIRECT_MESSAGE_CREATE: &str = "DIRECT_MESSAGE_CREATE";

pub const INTERACTION_CREATE: &str = "INTERACTION_CREATE";

pub const MESSAGE_AUDIT_PASS: &str = "MESSAGE_AUDIT_PASS";

pub const MESSAGE_AUDIT_REJECT: &str = "MESSAGE_AUDIT_REJECT";

pub const GROUP_JOIN_REQUEST: &str = "GROUP_JOIN_REQUEST";

pub const FRIEND_ADD: &str = "FRIEND_ADD";

pub const FRIEND_DEL: &str = "FRIEND_DEL";

pub const READY: &str = "READY";

pub const RESUMED: &str = "RESUMED";

pub const MESSAGE_EVENTS: [&str; 6] = [
    GROUP_AT_MESSAGE_CREATE,
    GROUP_MESSAGE_CREATE,
    C2C_MESSAGE_CREATE,
    AT_MESSAGE_CREATE,
    MESSAGE_CREATE,
    DIRECT_MESSAGE_CREATE,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCategory {
    Message,
    Interaction,
    Audit,
    Notice,
    Lifecycle,
    Unknown,
}

impl EventCategory {
    pub const fn label(self) -> &'static str {
        match self {
            EventCategory::Message => "消息",
            EventCategory::Interaction => "按钮交互",
            EventCategory::Audit => "审核回调",
            EventCategory::Notice => "通知",
            EventCategory::Lifecycle => "生命周期",
            EventCategory::Unknown => "未知",
        }
    }

    pub const fn dispatchable(self) -> bool {
        matches!(self, EventCategory::Message)
    }
}

pub fn category(event_type: Option<&str>) -> EventCategory {
    match event_type {
        None => EventCategory::Unknown,
        Some(name) if MESSAGE_EVENTS.contains(&name) => EventCategory::Message,
        Some(INTERACTION_CREATE) => EventCategory::Interaction,
        Some(MESSAGE_AUDIT_PASS) | Some(MESSAGE_AUDIT_REJECT) => EventCategory::Audit,
        Some(GROUP_JOIN_REQUEST)
        | Some(FRIEND_ADD)
        | Some(FRIEND_DEL)
        | Some("GROUP_MEMBER_ADD")
        | Some("GROUP_MEMBER_REMOVE")
        | Some("GROUP_ADD_ROBOT")
        | Some("GROUP_DEL_ROBOT")
        | Some("GROUP_MSG_RECEIVE")
        | Some("GROUP_MSG_REJECT")
        | Some("C2C_MSG_RECEIVE")
        | Some("C2C_MSG_REJECT") => EventCategory::Notice,
        Some(READY) | Some(RESUMED) => EventCategory::Lifecycle,
        Some(_) => EventCategory::Unknown,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageEvent {
    pub event_type: String,
    pub scene: Scene,
    pub target: Target,
    pub user: String,
    pub username: Option<String>,
    pub role: Option<RoleRequired>,
    pub content: String,
    pub raw_content: String,
    pub event_id: String,
    pub message_id: String,
    pub mentioned: bool,
    pub attachments: Vec<IncomingAttachment>,
    pub reference: Option<String>,
}

impl MessageEvent {
    pub fn is_private(&self) -> bool {
        matches!(self.scene, Scene::C2c | Scene::Dm)
    }

    pub fn to_incoming(&self) -> Incoming {
        let mut incoming = Incoming::new(
            self.scene,
            self.target.clone(),
            self.user.clone(),
            self.content.clone(),
        );
        incoming.username = self.username.clone();
        incoming.platform_role = self.role;
        incoming.private = self.is_private();
        incoming = incoming.with_attachments(self.attachments.clone());
        if let Some(reference) = &self.reference {
            incoming = incoming.with_reference(reference.clone());
        }
        incoming
    }

    pub fn dedup_key(&self) -> (&str, &str) {
        (self.event_id.as_str(), self.message_id.as_str())
    }

    pub fn parse(event: &ProtocolEvent) -> Option<Self> {
        let kind = event.event_type.as_deref()?;
        if !MESSAGE_EVENTS.contains(&kind) {
            return None;
        }
        let data = &event.data;
        let event_id = event.id.clone().unwrap_or_default();
        let message_id = text_of(data, "id").unwrap_or_default();
        match kind {
            GROUP_AT_MESSAGE_CREATE | GROUP_MESSAGE_CREATE => {
                let conversation = text_of(data, "group_openid")?;
                let author = data.get("author");
                let user = author.and_then(|author| {
                    first_text(author, &["member_openid", "union_openid", "id"])
                })?;
                let username = author
                    .and_then(|author| text_of(author, "username"))
                    .filter(|name| !name.is_empty());
                let role = author.and_then(role_of);
                let raw_content = text_of(data, "content").unwrap_or_default();
                let content = filter_at(&raw_content);
                Some(Self {
                    event_type: kind.to_string(),
                    scene: Scene::Group,
                    target: Target::new(Scene::Group, conversation).with_user(user.clone()),
                    user,
                    username,
                    role,
                    content,
                    raw_content,
                    event_id,
                    message_id,
                    mentioned: mentions_bot(data),
                    attachments: attachments_of(data),
                    reference: reference_of(data),
                })
            }
            C2C_MESSAGE_CREATE => {
                let author = data.get("author");
                let user = author.and_then(|author| {
                    first_text(author, &["user_openid", "union_openid", "id"])
                })?;
                let username = author
                    .and_then(|author| text_of(author, "username"))
                    .filter(|name| !name.is_empty());
                let role = author.and_then(role_of);
                let raw_content = text_of(data, "content").unwrap_or_default();
                let content = raw_content.trim().to_string();
                Some(Self {
                    event_type: kind.to_string(),
                    scene: Scene::C2c,
                    target: Target::new(Scene::C2c, user.clone()).with_user(user.clone()),
                    user,
                    username,
                    role,
                    content,
                    raw_content,
                    event_id,
                    message_id,
                    mentioned: true,
                    attachments: attachments_of(data),
                    reference: reference_of(data),
                })
            }
            AT_MESSAGE_CREATE | MESSAGE_CREATE | DIRECT_MESSAGE_CREATE => {
                let scene = match kind {
                    DIRECT_MESSAGE_CREATE => Scene::Dm,
                    _ => Scene::Channel,
                };
                let conversation = text_of(data, "channel_id")
                    .filter(|id| !id.is_empty())
                    .or_else(|| text_of(data, "guild_id").filter(|id| !id.is_empty()))?;
                let author = data.get("author");
                let user = author
                    .and_then(|author| first_text(author, &["union_openid", "id"]))
                    .or_else(|| text_of(data, "user_id"))?;
                let username = author
                    .and_then(|author| text_of(author, "username"))
                    .filter(|name| !name.is_empty());
                let role = author.and_then(role_of);
                let mention_id = author
                    .and_then(|author| text_of(author, "id"))
                    .unwrap_or_default();
                let mentioned = mentions_bot(data);
                if kind == MESSAGE_CREATE && !mentioned {
                    return None;
                }
                let raw_content = text_of(data, "content").unwrap_or_default();
                let content = strip_mentions(&raw_content, data);
                let content = if content.is_empty() && kind == AT_MESSAGE_CREATE {
                    strip_leading_mention(&raw_content)
                } else {
                    content
                };
                Some(Self {
                    event_type: kind.to_string(),
                    scene,
                    target: Target::new(scene, conversation)
                        .with_user(user.clone())
                        .with_mention_id(mention_id),
                    user,
                    username,
                    role,
                    content,
                    raw_content,
                    event_id,
                    message_id,
                    mentioned,
                    attachments: attachments_of(data),
                    reference: reference_of(data),
                })
            }
            _ => None,
        }
    }
}

fn attachments_of(data: &Value) -> Vec<IncomingAttachment> {
    let Some(list) = data.get("attachments").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|item| {
            let url = item.get("url").and_then(Value::as_str)?.trim();
            if url.is_empty() {
                return None;
            }
            let mut attachment = IncomingAttachment::new(
                url,
                item.get("content_type")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
            attachment.filename = item
                .get("filename")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            attachment.width = item
                .get("width")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u32::MAX as u64) as u32;
            attachment.height = item
                .get("height")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u32::MAX as u64) as u32;
            Some(attachment)
        })
        .collect()
}

fn reference_of(data: &Value) -> Option<String> {
    let reference = data
        .get("message_reference")
        .and_then(|value| value.get("message_id"))
        .and_then(Value::as_str)
        .or_else(|| data.get("reply_to").and_then(Value::as_str))
        .or_else(|| data.get("referenced_message_id").and_then(Value::as_str))
        .or_else(|| data.get("msg_id").and_then(Value::as_str));
    match reference.map(str::trim) {
        Some(id) if !id.is_empty() => Some(id.to_string()),
        _ => None,
    }
}

pub fn mention_tag(scene: Scene, user: &str) -> String {
    if user.trim().is_empty() {
        return String::new();
    }
    match scene.guild() {
        true => format!("<@!{user}>"),
        false => format!("<@{user}>"),
    }
}

pub fn markdown_mention(scene: Scene, user: &str) -> String {
    let user = user.trim();
    if user.is_empty() || !matches!(scene, Scene::Group | Scene::Channel) {
        return String::new();
    }
    format!("<qqbot-at-user id=\"{user}\" />")
}
pub const AT_AVATAR_FETCH_PX: i32 = 140;


pub const AT_AVATAR_SHOW_PX: i32 = 30;

pub fn avatar_url(app_id: &str, user: &str, px: i32) -> String {
    format!("https://thirdqq.qlogo.cn/qqapp/{app_id}/{user}/{px}")
}

pub fn avatar_mention(scene: Scene, app_id: &str, user: &str) -> String {
    let tag = markdown_mention(scene, user);
    if tag.is_empty() {
        return String::new();
    }
    if app_id.is_empty() {
        return tag;
    }
    format!(
        "> ### ![cornerRadius=100 #{px}px #{px}px]({url}) {tag}\n",
        px = AT_AVATAR_SHOW_PX,
        url = avatar_url(app_id, user, AT_AVATAR_FETCH_PX)
    )
}

pub fn filter_at(raw: &str) -> String {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("<@") {
        if let Some(index) = rest.find('>') {
            return rest[index + 1..]
                .trim_start_matches([' ', '\t', '\n'])
                .to_string();
        }
    }
    if let Some(rest) = trimmed.strip_prefix('@') {
        return match rest.find([' ', '\t', '\n']) {
            Some(index) => rest[index..]
                .trim_start_matches([' ', '\t', '\n'])
                .to_string(),
            None => String::new(),
        };
    }
    raw.to_string()
}

pub fn strip_mentions(content: &str, data: &Value) -> String {
    let mut current = content.to_string();
    for mention in mentions(data) {
        if mention.0 != Some(true) {
            continue;
        }
        match mention.1 {
            Some(id) if !id.is_empty() => {
                current = current.replace(&format!("<@!{id}>"), "");
                current = current.replace(&format!("<@{id}>"), "");
            }
            _ => {
                current = strip_leading_mention(&current);
            }
        }
    }
    current.trim().to_string()
}

pub fn strip_leading_mention(content: &str) -> String {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix("<@") {
        if let Some(index) = rest.find('>') {
            return rest[index + 1..].trim().to_string();
        }
    }
    trimmed.trim().to_string()
}

pub fn mentions_bot(data: &Value) -> bool {
    mentions(data)
        .into_iter()
        .any(|mention| mention.0 == Some(true))
}

fn mentions(data: &Value) -> Vec<(Option<bool>, Option<String>)> {
    let Some(items) = data.get("mentions").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .map(|item| {
            let is_you = item.get("is_you").and_then(Value::as_bool).or_else(|| {
                item.get("is_you")
                    .and_then(|value| value.as_str())
                    .map(|text| text == "true")
            });
            let id = first_text(item, &["id", "member_openid", "user_openid"]);
            (is_you, id)
        })
        .collect()
}

fn role_of(author: &Value) -> Option<RoleRequired> {
    first_text(author, &["member_role", "role"]).and_then(|role| role.parse().ok())
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    let raw = value.get(key)?;
    match raw {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn first_text(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = text_of(value, key) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(event_type: &str, data: serde_json::Value) -> ProtocolEvent {
        ProtocolEvent {
            id: Some(String::from("evt-1")),
            op: 0,
            event_type: Some(event_type.to_string()),
            seq: Some(1),
            data,
            raw: String::new(),
        }
    }

    #[test]
    fn filter_at_drops_exactly_the_mention_tag() {
        assert_eq!(filter_at("<@!bot1> /echo hi"), "/echo hi");
        assert_eq!(filter_at("<@bot1> /echo hi"), "/echo hi");
        assert_eq!(filter_at("<@!1234567890>/help"), "/help");
        assert_eq!(filter_at("  <@!bot1>   你好"), "你好");
        assert_eq!(filter_at("普通消息"), "普通消息");
        assert_eq!(filter_at("<@!bot1"), "<@!bot1");
    }

    #[test]
    fn avatar_mention_puts_a_round_avatar_before_the_tag() {
        assert_eq!(
            avatar_mention(Scene::Group, "app1", "u1"),
            "> ### ![cornerRadius=100 #30px #30px](https://thirdqq.qlogo.cn/qqapp/app1/u1/140) <qqbot-at-user id=\"u1\" />\n"
        );
        assert_eq!(
            avatar_mention(Scene::Group, "", "u1"),
            "<qqbot-at-user id=\"u1\" />"
        );
        assert_eq!(avatar_mention(Scene::Group, "app1", ""), "");
        assert_eq!(avatar_mention(Scene::C2c, "app1", "u1"), "");
    }

    #[test]
    fn c2c_message_is_always_treated_as_addressed() {
        let parsed = MessageEvent::parse(&event(
            "C2C_MESSAGE_CREATE",
            json!({
                "id": "m1",
                "content": "hello",
                "author": { "user_openid": "u1", "username": "小明" }
            }),
        ))
        .expect("私聊事件要能解析");
        assert_eq!(parsed.scene, Scene::C2c);
        assert!(parsed.mentioned, "私聊一定是对机器人说的");
        assert_eq!(parsed.content, "hello");
    }

    #[test]
    fn group_message_mentioned_flag_follows_the_mentions_array() {
        let plain = MessageEvent::parse(&event(
            "GROUP_AT_MESSAGE_CREATE",
            json!({
                "id": "m1",
                "content": "hi",
                "group_openid": "g1",
                "author": { "member_openid": "u1" }
            }),
        ))
        .expect("群事件要能解析");
        assert!(!plain.mentioned, "没有 mentions 数组时为 false");
        let at_bot = MessageEvent::parse(&event(
            "GROUP_AT_MESSAGE_CREATE",
            json!({
                "id": "m2",
                "content": "<@!bot1> hi",
                "group_openid": "g1",
                "author": { "member_openid": "u1" },
                "mentions": [{ "id": "bot1", "is_you": true }]
            }),
        ))
        .expect("群事件要能解析");
        assert!(at_bot.mentioned, "点了机器人要为 true");
        assert_eq!(at_bot.content, "hi", "正文要去掉 at 标签");
    }

    #[test]
    fn channel_message_without_mention_is_dropped_but_at_message_is_kept() {
        let dropped = MessageEvent::parse(&event(
            "MESSAGE_CREATE",
            json!({
                "id": "m1",
                "content": "闲聊",
                "channel_id": "c1",
                "author": { "id": "42" }
            }),
        ));
        assert!(dropped.is_none(), "频道里没点名的消息要丢弃");
        let kept = MessageEvent::parse(&event(
            "AT_MESSAGE_CREATE",
            json!({
                "id": "m2",
                "content": "<@!bot1> /b30",
                "channel_id": "c1",
                "author": { "id": "42" },
                "mentions": [{ "id": "bot1", "is_you": true }]
            }),
        ))
        .expect("AT_MESSAGE_CREATE 必须保留");
        assert!(kept.mentioned);
        assert_eq!(kept.content, "/b30");
        assert_eq!(kept.scene, Scene::Channel);
    }

    #[test]
    fn direct_message_uses_dm_scene() {
        let parsed = MessageEvent::parse(&event(
            "DIRECT_MESSAGE_CREATE",
            json!({
                "id": "m1",
                "content": "hi",
                "guild_id": "g1",
                "channel_id": "c9",
                "author": { "id": "42" }
            }),
        ))
        .expect("私信要能解析");
        assert_eq!(parsed.scene, Scene::Dm);
        assert_eq!(parsed.target.id, "c9");
    }
}
