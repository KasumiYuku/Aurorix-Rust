use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use aurorix_core::{Scene, Target};

pub const GROUP_MESSAGE_PATH: &str = "/v2/groups/{group_openid}/messages";
pub const USER_MESSAGE_PATH: &str = "/v2/users/{openid}/messages";
pub const CHANNEL_MESSAGE_PATH: &str = "/channels/{channel_id}/messages";
pub const DM_MESSAGE_PATH: &str = "/dms/{guild_id}/messages";
pub const APP_ACCESS_TOKEN_PATH: &str = "/app/getAppAccessToken";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageKind {
    PlainText = 0,
    Markdown = 2,
    Media = 7,
}

impl MessageKind {
    pub const ALL: [MessageKind; 3] = [
        MessageKind::PlainText,
        MessageKind::Markdown,
        MessageKind::Media,
    ];

    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(MessageKind::PlainText),
            2 => Some(MessageKind::Markdown),
            7 => Some(MessageKind::Media),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            MessageKind::PlainText => "纯文本",
            MessageKind::Markdown => "markdown",
            MessageKind::Media => "媒体",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendRequest {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    pub msg_type: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_seq: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_reference: Option<MessageReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReference {
    pub message_id: String,
    #[serde(default)]
    pub ignore_get_message_error: bool,
}

impl MessageReference {
    pub fn new(message_id: impl Into<String>) -> Self {
        Self {
            message_id: message_id.into(),
            ignore_get_message_error: false,
        }
    }

    pub fn ignoring_missing(mut self, ignore: bool) -> Self {
        self.ignore_get_message_error = ignore;
        self
    }
}

impl SendRequest {
    pub fn new(content: impl Into<String>, kind: MessageKind) -> Self {
        Self {
            content: content.into(),
            msg_type: kind.code(),
            msg_id: None,
            msg_seq: None,
            event_id: None,
            markdown: None,
            keyboard: None,
            media: None,
            timestamp: None,
            message_reference: None,
        }
    }

    pub fn plain(content: impl Into<String>) -> Self {
        Self::new(content, MessageKind::PlainText)
    }

    pub fn markdown(content: impl Into<String>, markdown: Value) -> Self {
        let mut request = Self::new(content, MessageKind::Markdown);
        request.markdown = Some(markdown);
        request
    }

    pub fn media(content: impl Into<String>, media: Value) -> Self {
        let mut request = Self::new(content, MessageKind::Media);
        request.media = Some(media);
        request
    }

    pub fn with_msg_id(mut self, msg_id: impl Into<String>) -> Self {
        self.msg_id = Some(msg_id.into());
        self
    }

    pub fn with_msg_seq(mut self, msg_seq: u32) -> Self {
        self.msg_seq = Some(msg_seq);
        self
    }

    pub fn with_event_id(mut self, event_id: impl Into<String>) -> Self {
        self.event_id = Some(event_id.into());
        self
    }

    pub fn with_keyboard(mut self, keyboard: Value) -> Self {
        self.keyboard = Some(keyboard);
        self
    }

    pub fn with_reference(mut self, message_id: impl Into<String>) -> Self {
        self.message_reference = Some(MessageReference::new(message_id));
        self
    }

    pub fn with_timestamp(mut self, timestamp: i64) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    pub fn kind(&self) -> Option<MessageKind> {
        MessageKind::from_code(self.msg_type)
    }

    pub fn path_for_scene(scene: Scene, id: &str) -> String {
        match scene {
            Scene::Group => format!("/v2/groups/{id}/messages"),
            Scene::C2c => format!("/v2/users/{id}/messages"),
            Scene::Channel => format!("/channels/{id}/messages"),
            Scene::Dm => format!("/dms/{id}/messages"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_kind_codes_match_platform() {
        assert_eq!(MessageKind::PlainText.code(), 0);
        assert_eq!(MessageKind::Markdown.code(), 2);
        assert_eq!(MessageKind::Media.code(), 7);
        assert_eq!(MessageKind::ALL.len(), 3);
        for kind in MessageKind::ALL {
            assert_eq!(
                MessageKind::from_code(kind.code()),
                Some(kind),
                "消息类型往返失败"
            );
            assert!(!kind.label().is_empty());
        }
        assert_eq!(MessageKind::from_code(1), None);
        assert_eq!(MessageKind::from_code(8), None);
    }

    #[test]
    fn every_scene_maps_to_its_platform_path() {
        assert_eq!(
            SendRequest::path_for_scene(Scene::Group, "g1"),
            "/v2/groups/g1/messages"
        );
        assert_eq!(
            SendRequest::path_for_scene(Scene::C2c, "u1"),
            "/v2/users/u1/messages"
        );
        assert_eq!(
            SendRequest::path_for_scene(Scene::Channel, "c1"),
            "/channels/c1/messages"
        );
        assert_eq!(
            SendRequest::path_for_scene(Scene::Dm, "guild1"),
            "/dms/guild1/messages"
        );
        assert_eq!(GROUP_MESSAGE_PATH, "/v2/groups/{group_openid}/messages");
        assert_eq!(USER_MESSAGE_PATH, "/v2/users/{openid}/messages");
        assert_eq!(CHANNEL_MESSAGE_PATH, "/channels/{channel_id}/messages");
        assert_eq!(DM_MESSAGE_PATH, "/dms/{guild_id}/messages");
        assert_eq!(APP_ACCESS_TOKEN_PATH, "/app/getAppAccessToken");
    }

    #[test]
    fn plain_request_omits_empty_fields() {
        let request = SendRequest::plain("你好");
        let text = serde_json::to_string(&request).expect("序列化要成功");
        assert_eq!(text, r#"{"content":"你好","msg_type":0}"#);
        let value = serde_json::to_value(&request).expect("序列化要成功");
        for key in [
            "msg_id",
            "msg_seq",
            "event_id",
            "markdown",
            "keyboard",
            "media",
            "timestamp",
        ] {
            assert!(value.get(key).is_none(), "字段 {key} 不该出现");
        }
        assert_eq!(request.kind(), Some(MessageKind::PlainText));
    }

    #[test]
    fn full_request_keeps_all_platform_fields() {
        let request = SendRequest::markdown("", json!({"content": [{"type": "text"}]}))
            .with_msg_id("m1")
            .with_msg_seq(3)
            .with_event_id("e1")
            .with_keyboard(json!({"rows": []}))
            .with_timestamp(1700000000);
        let value = serde_json::to_value(&request).expect("序列化要成功");
        assert!(
            value.get("content").is_none(),
            "markdown 报文不发空 content, 与平台一致"
        );
        assert_eq!(value["msg_type"], 2);
        assert_eq!(value["msg_id"], "m1");
        assert_eq!(value["msg_seq"], 3);
        assert_eq!(value["event_id"], "e1");
        assert_eq!(value["markdown"]["content"][0]["type"], "text");
        assert_eq!(value["keyboard"]["rows"].as_array().map(Vec::len), Some(0));
        assert_eq!(value["timestamp"], 1700000000);
        assert!(value.get("media").is_none());
        assert_eq!(request.kind(), Some(MessageKind::Markdown));
    }

    #[test]
    fn media_and_round_trip_are_stable() {
        let request = SendRequest::media("图片", json!({"file_info": "f1"}));
        assert_eq!(request.msg_type, 7);
        let text = serde_json::to_string(&request).expect("序列化要成功");
        let parsed: SendRequest = serde_json::from_str(&text).expect("反序列化要成功");
        assert_eq!(parsed, request);
        assert_eq!(parsed.media, Some(json!({"file_info": "f1"})));
    }
}
