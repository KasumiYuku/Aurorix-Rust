use aurorix_core::{Scene, Target};
use aurorix_gateway::ProtocolEvent;
use serde_json::Value;

use crate::event::INTERACTION_CREATE;

pub const INTERACTION_TYPE_BUTTON: i64 = 11;

#[derive(Debug, Clone, PartialEq)]
pub struct InteractionEvent {
    pub event_id: String,
    pub interaction_id: String,
    pub button_id: String,
    pub button_data: String,
    pub user: String,
    pub scene: Scene,
    pub target: Target,
    pub timestamp: Option<String>,
    pub raw: Value,
}

    pub fn interaction_id(event: &ProtocolEvent) -> String {
        if let Some(id) = text_of(&event.data, "id").filter(|id| !id.is_empty()) {
            return id;
        }
        match event.id.as_deref() {
            Some(id) => match id.rsplit_once(':') {
                Some((_, tail)) if !tail.is_empty() => tail.to_string(),
                _ => id.to_string(),
            },
            None => String::new(),
        }
    }

impl InteractionEvent {
    pub fn parse(event: &ProtocolEvent) -> Option<Self> {
        if event.event_type.as_deref() != Some(INTERACTION_CREATE) {
            return None;
        }
        let data = &event.data;
        if interaction_type(data).is_some_and(|kind| kind != INTERACTION_TYPE_BUTTON) {
            return None;
        }
        let event_id = event.id.clone().unwrap_or_default();
        let interaction_id = text_of(data, "id").unwrap_or_default();
        let resolved = resolved_of(data);
        let button_id = resolved
            .and_then(|inner| text_of(inner, "button_id"))
            .unwrap_or_default();
        let button_data = resolved
            .and_then(|inner| text_of(inner, "button_data"))
            .unwrap_or_default();
        if button_id.is_empty() && button_data.is_empty() {
            return None;
        }
        let user = resolved
            .and_then(|inner| text_of(inner, "user_id"))
            .or_else(|| first_text(data, &["group_member_openid", "user_openid", "user_id"]))
            .unwrap_or_default();
        let chat_type = number_of(data, "chat_type");
        let scene_name = text_of(data, "scene").filter(|name| !name.is_empty());
        let scene = Self::scene_of(chat_type, scene_name.as_deref())?;
        let target_id = match scene {
            Scene::Group => text_of(data, "group_openid").filter(|id| !id.is_empty()),
            Scene::Channel => first_text(data, &["channel_id", "guild_id"]),
            Scene::C2c | Scene::Dm => {
                first_text(data, &["user_openid", "user_id", "union_openid"]).or_else(|| {
                    resolved
                        .and_then(|inner| text_of(inner, "user_id"))
                        .filter(|id| !id.is_empty())
                })
            }
        }?;
        Some(Self {
            event_id,
            interaction_id,
            button_id,
            button_data,
            user: user.clone(),
            scene,
            target: Target::new(scene, target_id).with_user(user),
            timestamp: text_of(data, "timestamp"),
            raw: data.clone(),
        })
    }

    pub fn scene_of(chat_type: Option<i64>, scene: Option<&str>) -> Option<Scene> {
        match scene {
            Some("group") => return Some(Scene::Group),
            Some("c2c") => return Some(Scene::C2c),
            Some("guild") | Some("channel") => return Some(Scene::Channel),
            Some("dm") | Some("direct") => return Some(Scene::Dm),
            _ => {}
        }
        match chat_type? {
            1 => Some(Scene::Group),
            2 => Some(Scene::C2c),
            3 => Some(Scene::Channel),
            _ => None,
        }
    }
}

fn resolved_of(data: &Value) -> Option<&Value> {
    let payload = data.get("data");
    payload
        .and_then(|inner| inner.get("resolved"))
        .or_else(|| {
            payload
                .and_then(|inner| inner.get("callback"))
                .and_then(|callback| callback.get("resolved"))
        })
        .or_else(|| data.get("resolved"))
        .or_else(|| {
            data.get("callback")
                .and_then(|callback| callback.get("resolved"))
        })
}

fn interaction_type(data: &Value) -> Option<i64> {
    number_of(data, "type")
}

fn number_of(value: &Value, key: &str) -> Option<i64> {
    let raw = value.get(key)?;
    match raw {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
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

    const GROUP_RAW: &str = r#"{"application_id":"102108865","chat_type":1,"data":{"resolved":{"button_data":"50","button_id":"arcaea.score.best50"},"type":11},"group_member_openid":"3C23665AD14B72421F84EABE461BCAB0","group_openid":"EDB6B531E7ED22C3F6178D70F8085FF4","id":"544deb3c-864b-4439-8d97-3e0107d312d9","scene":"group","timestamp":"2026-11-26T17:20:27+08:00","type":11,"version":1}"#;

    const C2C_RAW: &str = r#"{"application_id":"102108865","chat_type":2,"data":{"resolved":{"button_data":"menu","button_id":"cn.arcaea.common.menu"},"type":11},"id":"431d43f0-d11b-466e-8365-5cb607b98ceb","scene":"c2c","timestamp":"2026-11-26T17:23:40+08:00","type":11,"union_openid":"590972AAC79AC054156891E1E28FDDB9","user_openid":"590972AAC79AC054156891E1E28FDDB9","version":1}"#;

    fn event(raw: &str) -> ProtocolEvent {
        ProtocolEvent {
            id: Some("INTERACTION_CREATE:544deb3c-864b-4439-8d97-3e0107d312d9".to_string()),
            op: 0,
            event_type: Some(INTERACTION_CREATE.to_string()),
            seq: None,
            data: serde_json::from_str(raw).expect("payload"),
            raw: raw.to_string(),
        }
    }

    #[test]
    fn group_click_resolves_group_target() {
        let parsed = InteractionEvent::parse(&event(GROUP_RAW)).expect("群按钮要能解析");
        assert_eq!(parsed.scene, Scene::Group);
        assert_eq!(parsed.button_id, "arcaea.score.best50");
        assert_eq!(parsed.button_data, "50");
        assert_eq!(parsed.target.id, "EDB6B531E7ED22C3F6178D70F8085FF4");
        assert_eq!(parsed.user, "3C23665AD14B72421F84EABE461BCAB0");
        assert_eq!(parsed.interaction_id, "544deb3c-864b-4439-8d97-3e0107d312d9");
    }

    #[test]
    fn c2c_click_resolves_user_target() {
        let parsed = InteractionEvent::parse(&event(C2C_RAW)).expect("单聊按钮要能解析");
        assert_eq!(parsed.scene, Scene::C2c);
        assert_eq!(parsed.button_id, "cn.arcaea.common.menu");
        assert_eq!(parsed.target.id, "590972AAC79AC054156891E1E28FDDB9");
        assert_eq!(parsed.user, "590972AAC79AC054156891E1E28FDDB9");
        assert_eq!(parsed.interaction_id, "431d43f0-d11b-466e-8365-5cb607b98ceb");
    }

    #[test]
    fn chat_type_maps_interaction_semantics() {
        assert_eq!(InteractionEvent::scene_of(Some(1), None), Some(Scene::Group));
        assert_eq!(InteractionEvent::scene_of(Some(2), None), Some(Scene::C2c));
        assert_eq!(InteractionEvent::scene_of(Some(3), None), Some(Scene::Channel));
        assert_eq!(InteractionEvent::scene_of(Some(9), None), None);
        assert_eq!(
            InteractionEvent::scene_of(Some(1), Some("c2c")),
            Some(Scene::C2c)
        );
    }
}
