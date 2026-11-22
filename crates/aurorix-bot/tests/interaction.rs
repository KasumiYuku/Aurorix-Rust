use aurorix_bot::{InteractionEvent, Scene, INTERACTION_TYPE_BUTTON};
use aurorix_gateway::ProtocolEvent;
use serde_json::{json, Value};

fn event(event_type: &str, data: Value) -> ProtocolEvent {
    ProtocolEvent {
        id: Some(String::from("evt-1")),
        op: 0,
        event_type: Some(event_type.to_string()),
        seq: Some(7),
        data,
        raw: String::new(),
    }
}

fn interaction(data: Value) -> ProtocolEvent {
    event("INTERACTION_CREATE", data)
}

#[test]
fn group_button_click_resolves_group_target() {
    let parsed = InteractionEvent::parse(&interaction(json!({
        "id": "interaction-id",
        "type": INTERACTION_TYPE_BUTTON,
        "scene": "group",
        "chat_type": 1,
        "timestamp": "2024-01-01T00:00:00+08:00",
        "group_openid": "G1",
        "channel_id": "",
        "guild_id": "",
        "data": {
            "type": INTERACTION_TYPE_BUTTON,
            "resolved": {
                "button_data": "7",
                "button_id": "aurorix.footer.7",
                "user_id": "U1"
            }
        }
    })))
    .expect("群聊按钮点击必须能解析");
    assert_eq!(parsed.scene, Scene::Group, "chat_type=1 要判定成群聊");
    assert_eq!(
        parsed.target.scene,
        Scene::Group,
        "目标场景要跟事件场景一致"
    );
    assert_eq!(parsed.target.id, "G1", "群聊目标要取 group_openid");
    assert_eq!(parsed.target.user, "U1", "目标要带上点击者");
    assert_eq!(parsed.button_id, "aurorix.footer.7", "按钮 id 要解析出来");
    assert_eq!(parsed.button_data, "7", "按钮数据要解析出来");
    assert_eq!(parsed.user, "U1", "点击者要取 resolved.user_id");
    assert_eq!(
        parsed.interaction_id, "interaction-id",
        "交互 id 要取 data.id"
    );
    assert_eq!(parsed.event_id, "evt-1", "事件 id 要取顶层 id");
    assert_eq!(
        parsed.timestamp.as_deref(),
        Some("2024-01-01T00:00:00+08:00"),
        "时间戳要原样保留"
    );
    assert_eq!(
        parsed.raw.get("group_openid").and_then(Value::as_str),
        Some("G1"),
        "原始数据要保留下来"
    );
}

#[test]
fn c2c_button_click_uses_user_as_target() {
    let parsed = InteractionEvent::parse(&interaction(json!({
        "id": "interaction-c2c",
        "type": INTERACTION_TYPE_BUTTON,
        "scene": "c2c",
        "chat_type": 2,
        "data": {
            "resolved": { "button_id": "aurorix.menu.1", "button_data": "1", "user_id": "U1" }
        }
    })))
    .expect("单聊按钮点击必须能解析");
    assert_eq!(parsed.scene, Scene::C2c, "chat_type=2 要判定成单聊");
    assert_eq!(parsed.target.scene, Scene::C2c, "目标场景要是单聊");
    assert_eq!(
        parsed.target.id, "U1",
        "单聊没有 group_openid 时目标要回落到 resolved.user_id"
    );
    assert_eq!(parsed.target.user, "U1", "目标要带上点击者");
    assert_eq!(parsed.button_data, "1", "按钮数据要解析出来");
    assert!(parsed.timestamp.is_none(), "没有 timestamp 时要为 None");
}

#[test]
fn channel_and_direct_message_scenes_are_distinguished() {
    let channel = InteractionEvent::parse(&interaction(json!({
        "id": "interaction-channel",
        "type": INTERACTION_TYPE_BUTTON,
        "chat_type": 3,
        "channel_id": "C9",
        "guild_id": "GUILD1",
        "data": {
            "resolved": { "button_id": "b.channel", "user_id": "U2" }
        }
    })))
    .expect("子频道按钮点击必须能解析");
    assert_eq!(channel.scene, Scene::Channel, "chat_type=3 要判定成子频道");
    assert_eq!(channel.target.id, "C9", "频道目标要优先取 channel_id");

    let direct = InteractionEvent::parse(&interaction(json!({
        "id": "interaction-dm",
        "type": INTERACTION_TYPE_BUTTON,
        "scene": "dm",
        "chat_type": 3,
        "channel_id": "C10",
        "guild_id": "GUILD1",
        "data": {
            "resolved": { "button_id": "b.dm", "user_id": "U3" }
        }
    })))
    .expect("频道私信按钮点击必须能解析");
    assert_eq!(direct.scene, Scene::Dm, "scene=dm 要判定成频道私信");
    assert_eq!(direct.target.id, "U3", "频道私信目标要用点击者 id");
}

#[test]
fn channel_falls_back_to_guild_id_when_channel_id_missing() {
    let parsed = InteractionEvent::parse(&interaction(json!({
        "id": "interaction-guild",
        "type": INTERACTION_TYPE_BUTTON,
        "chat_type": 3,
        "channel_id": "",
        "guild_id": "GUILD7",
        "data": {
            "resolved": { "button_id": "b.guild", "user_id": "U4" }
        }
    })))
    .expect("缺 channel_id 的频道点击要能解析");
    assert_eq!(
        parsed.target.id, "GUILD7",
        "channel_id 为空时要回落到 guild_id"
    );
}

#[test]
fn guild_scene_name_is_used_when_chat_type_missing() {
    let parsed = InteractionEvent::parse(&interaction(json!({
        "id": "interaction-fallback",
        "type": INTERACTION_TYPE_BUTTON,
        "scene": "guild",
        "channel_id": "C11",
        "data": {
            "resolved": { "button_id": "b.fallback", "button_data": "2", "user_id": "U5" }
        }
    })))
    .expect("chat_type 缺失时要靠 scene 兜底解析");
    assert_eq!(parsed.scene, Scene::Channel, "scene=guild 要判定成频道");
    assert_eq!(parsed.target.id, "C11", "频道目标要取 channel_id");
}

#[test]
fn missing_resolved_or_empty_buttons_are_dropped() {
    let no_resolved = InteractionEvent::parse(&interaction(json!({
        "id": "interaction-bare",
        "type": INTERACTION_TYPE_BUTTON,
        "chat_type": 1,
        "group_openid": "G1",
        "data": {}
    })));
    assert!(
        no_resolved.is_none(),
        "没有 resolved 时无法分发按钮，必须丢弃"
    );

    let empty_buttons = InteractionEvent::parse(&interaction(json!({
        "id": "interaction-empty",
        "type": INTERACTION_TYPE_BUTTON,
        "chat_type": 1,
        "group_openid": "G1",
        "data": {
            "resolved": { "button_id": "", "button_data": "", "user_id": "U1" }
        }
    })));
    assert!(
        empty_buttons.is_none(),
        "button_id 与 button_data 同时为空时必须丢弃"
    );
}

#[test]
fn non_interaction_event_type_is_rejected() {
    let other = InteractionEvent::parse(&event(
        "GROUP_AT_MESSAGE_CREATE",
        json!({
            "id": "m1",
            "chat_type": 0,
            "group_openid": "G1",
            "data": { "resolved": { "button_id": "b", "button_data": "1" } }
        }),
    ));
    assert!(other.is_none(), "不是 INTERACTION_CREATE 的事件必须丢弃");

    let no_type = InteractionEvent::parse(&event(
        "",
        json!({
            "id": "m1",
            "chat_type": 0,
            "group_openid": "G1",
            "data": { "resolved": { "button_id": "b", "button_data": "1" } }
        }),
    ));
    assert!(no_type.is_none(), "缺 event_type 的事件必须丢弃");
}

#[test]
fn non_button_interaction_type_is_rejected() {
    let parsed = InteractionEvent::parse(&interaction(json!({
        "id": "interaction-other",
        "type": 12,
        "chat_type": 1,
        "group_openid": "G1",
        "data": {
            "resolved": { "button_id": "b", "button_data": "1", "user_id": "U1" }
        }
    })));
    assert!(parsed.is_none(), "非按钮类型的交互不能当作按钮点击分发");
}

#[test]
fn scene_of_rejects_unknown_chat_type_and_scene() {
    assert!(
        InteractionEvent::scene_of(None, None).is_none(),
        "既没有 chat_type 也没有 scene 时必须返回 None"
    );
    assert!(
        InteractionEvent::scene_of(Some(9), Some("wechat")).is_none(),
        "chat_type 与 scene 都认不出来时必须返回 None"
    );
    assert_eq!(
        InteractionEvent::scene_of(Some(1), Some("c2c")),
        Some(Scene::C2c),
        "scene 优先于 chat_type"
    );
    assert_eq!(
        InteractionEvent::scene_of(None, Some("c2c")),
        Some(Scene::C2c),
        "缺 chat_type 时按 scene 判定单聊"
    );
    assert_eq!(
        InteractionEvent::scene_of(None, Some("guild")),
        Some(Scene::Channel),
        "缺 chat_type 时按 scene 判定频道"
    );
    assert_eq!(
        InteractionEvent::scene_of(Some(1), None),
        Some(Scene::Group),
        "chat_type=1 是群聊"
    );
    assert_eq!(
        InteractionEvent::scene_of(Some(2), None),
        Some(Scene::C2c),
        "chat_type=2 是单聊"
    );
    assert_eq!(
        InteractionEvent::scene_of(Some(3), None),
        Some(Scene::Channel),
        "chat_type=3 是频道"
    );
    assert_eq!(
        InteractionEvent::scene_of(Some(0), None),
        None,
        "chat_type=0 认不出来"
    );
}
