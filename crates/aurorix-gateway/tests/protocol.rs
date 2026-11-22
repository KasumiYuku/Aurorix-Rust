mod common;

use std::time::Duration;

use aurorix_gateway::intents::{GUILD_MESSAGES, INTERACTIONS, MESSAGE_AUDIT, USER_MESSAGE};
use aurorix_gateway::{
    auth_token, bits_for_names, bits_for_set, can_resume, classify, double, ensure_protocol,
    heartbeat_frame, heartbeat_interval_ms, identify_frame, intent_bit, is_permanent, jitter,
    need_reidentify, parse_hello, parse_ready, requires_websocket, resume_frame, Backoff,
    CloseAction, GatewayState, GatewayStats, Protocol, ProtocolEvent, BASE_DELAY,
    DEFAULT_INTENT_BITS, MAX_DELAY,
};
use aurorix_protocol::{EventKind, IntentSet};

#[test]
fn backoff_doubles_from_one_second_and_caps_at_thirty() {
    let mut backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(30));
    let mut seen = Vec::new();
    for _ in 0..8 {
        seen.push(backoff.advance().as_millis() as u64);
    }
    assert_eq!(
        seen,
        vec![1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000],
        "退避序列必须是 1s 起翻倍并封顶 30s"
    );
    assert!(backoff.current() <= backoff.max(), "当前退避不能越过上限");
}

#[test]
fn backoff_caps_on_exact_boundary() {
    let mut backoff = Backoff::new(Duration::from_millis(8), Duration::from_millis(16));
    assert_eq!(backoff.advance(), Duration::from_millis(8));
    assert_eq!(
        backoff.current(),
        Duration::from_millis(16),
        "翻倍正好等于上限要封顶"
    );
    assert_eq!(backoff.advance(), Duration::from_millis(16));
    assert_eq!(
        backoff.current(),
        Duration::from_millis(16),
        "封顶之后不能继续翻倍"
    );
    assert_eq!(
        double(Duration::from_millis(16), Duration::from_millis(16)),
        Duration::from_millis(16)
    );
}

#[test]
fn backoff_resets_after_healthy_connection() {
    let mut backoff = Backoff::default_policy();
    assert_eq!(backoff.current(), BASE_DELAY);
    assert_eq!(backoff.max(), MAX_DELAY);
    backoff.advance();
    backoff.advance();
    assert!(backoff.current() > BASE_DELAY, "连续失败后应当变长");
    backoff.reset();
    assert_eq!(backoff.current(), BASE_DELAY, "健康一次必须复位到 1s");
}

#[test]
fn jitter_keeps_quarter_amplitude() {
    for _ in 0..64 {
        let value = jitter(Duration::from_millis(1000)).as_millis() as u64;
        assert!((750..1250).contains(&value), "抖动幅度必须是上下四分之一");
    }
    assert_eq!(
        jitter(Duration::from_millis(1)),
        Duration::from_millis(1),
        "小于 2ms 不做抖动"
    );
}

#[test]
fn close_codes_are_split_three_ways() {
    assert_eq!(classify(4914), CloseAction::Stop, "4914 必须永久停止重连");
    assert_eq!(classify(4915), CloseAction::Stop, "4915 必须永久停止重连");
    assert_eq!(classify(4004), CloseAction::AuthFail, "4004 必须作废 token");
    assert_eq!(classify(4006), CloseAction::Reidentify, "4006 必须清会话");
    assert_eq!(classify(4007), CloseAction::Reidentify, "4007 必须清会话");
    assert_eq!(classify(1006), CloseAction::Reconnect, "其余走抖动重连");
    assert!(!can_resume(4914), "4914 不可恢复");
    assert!(!can_resume(4915), "4915 不可恢复");
    assert!(can_resume(4004), "4004 可以重新鉴权");
    assert!(can_resume(4006), "4006 可以重新鉴权");
    assert!(need_reidentify(4004));
    assert!(need_reidentify(4006));
    assert!(need_reidentify(4007));
    assert!(!need_reidentify(4914));
    assert!(!need_reidentify(1006));
    assert!(is_permanent(4915));
    assert!(!is_permanent(4006));
}

#[test]
fn intent_bits_follow_go_table() {
    assert_eq!(intent_bit("GROUP_AT_MESSAGE_CREATE"), 1 << 25);
    assert_eq!(intent_bit("INTERACTION_CREATE"), 1 << 26);
    assert_eq!(intent_bit("MESSAGE_CREATE"), 1 << 9);
    assert_eq!(intent_bit("AT_MESSAGE_CREATE"), 1 << 30);
    assert_eq!(intent_bit("GUILD_MEMBER_ADD"), 1 << 1);
    assert_eq!(intent_bit("MESSAGE_AUDIT_PASS"), 1 << 27);
    assert_eq!(intent_bit("UNKNOWN_EVENT"), 0, "未识别事件名贡献 0 位");
    assert_eq!(DEFAULT_INTENT_BITS, 251_658_240, "默认位必须与 Go 一致");
    assert_eq!(
        intent_bit("GROUP_AT_MESSAGE_CREATE") | intent_bit("INTERACTION_CREATE"),
        USER_MESSAGE | INTERACTIONS,
        "两个事件名按位或"
    );
}

#[test]
fn empty_subscription_falls_back_to_default_bits() {
    assert_eq!(bits_for_names(&[]), DEFAULT_INTENT_BITS);
    assert_eq!(
        bits_for_names(&["UNKNOWN_EVENT".to_string()]),
        DEFAULT_INTENT_BITS,
        "一个位都没编译出来时要回退默认位"
    );
    assert_eq!(
        bits_for_names(&["INTERACTION_CREATE".to_string()]),
        INTERACTIONS
    );
    assert_eq!(
        bits_for_names(&[
            "INTERACTION_CREATE".to_string(),
            "MESSAGE_AUDIT_PASS".to_string()
        ]),
        INTERACTIONS | MESSAGE_AUDIT
    );
}

#[test]
fn guild_subscriptions_are_rejected_on_webhook() {
    assert!(requires_websocket(&["MESSAGE_CREATE".to_string()]));
    assert!(requires_websocket(&["GUILD_CREATE".to_string()]));
    assert!(!requires_websocket(
        &["GROUP_AT_MESSAGE_CREATE".to_string()]
    ));
    assert!(
        ensure_protocol(Protocol::Webhook, &["MESSAGE_CREATE".to_string()]).is_err(),
        "webhook 订阅频道事件必须在启动期拒绝"
    );
    assert!(ensure_protocol(Protocol::Websocket, &["MESSAGE_CREATE".to_string()]).is_ok());
    assert!(ensure_protocol(Protocol::Webhook, &["GROUP_AT_MESSAGE_CREATE".to_string()]).is_ok());
    assert!(
        Protocol::Websocket.requires_websocket(&["GROUP_AT_MESSAGE_CREATE".to_string()]),
        "websocket 协议永远需要长连接"
    );
    assert!(
        Protocol::Webhook.requires_websocket(&["MESSAGE_CREATE".to_string()]),
        "webhook 订阅频道事件也必须走长连接"
    );
    assert!(
        !Protocol::Webhook.requires_websocket(&["GROUP_AT_MESSAGE_CREATE".to_string()]),
        "webhook 订阅群聊事件不需要长连接"
    );
    assert_eq!(Protocol::parse("websocket"), Some(Protocol::Websocket));
    assert_eq!(Protocol::parse("webhook"), Some(Protocol::Webhook));
    assert_eq!(Protocol::parse("mq"), None);
    assert_eq!(Protocol::Websocket.as_str(), "websocket");
}

#[test]
fn intent_set_from_protocol_crate_compiles_to_bits() {
    let set = IntentSet::from_names(&[
        "GROUP_AT_MESSAGE_CREATE".to_string(),
        "MESSAGE_CREATE".to_string(),
    ]);
    assert_eq!(
        bits_for_set(&set),
        USER_MESSAGE | GUILD_MESSAGES,
        "未知事件名也要参与位编译"
    );
}

#[test]
fn heartbeat_interval_is_capped_at_forty_five_seconds() {
    assert_eq!(heartbeat_interval_ms(30_000), 30_000);
    assert_eq!(heartbeat_interval_ms(45_000), 45_000);
    assert_eq!(heartbeat_interval_ms(45_001), 45_000);
    assert_eq!(heartbeat_interval_ms(999_999), 45_000, "超长间隔按 45s 走");
    let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 120_000}}).to_string();
    assert_eq!(parse_hello(&hello), Some(45_000));
    let ack = serde_json::json!({"op": 11}).to_string();
    assert_eq!(parse_hello(&ack), None);
    assert_eq!(parse_hello("不是 json"), None);
}

#[test]
fn identify_and_resume_carry_qqbot_token() {
    let identify: serde_json::Value =
        serde_json::from_str(&identify_frame("tok-1", DEFAULT_INTENT_BITS, [0, 1]))
            .expect("Identify 帧");
    assert_eq!(identify["op"], 2);
    assert_eq!(
        identify["d"]["token"], "QQBot tok-1",
        "token 必须带 QQBot 前缀"
    );
    assert_eq!(identify["d"]["shard"], serde_json::json!([0, 1]));
    assert_eq!(identify["d"]["intents"], DEFAULT_INTENT_BITS);
    assert_eq!(identify["d"]["properties"]["$os"], std::env::consts::OS);

    let resume: serde_json::Value =
        serde_json::from_str(&resume_frame("tok-1", "sess-1", 42, [0, 1])).expect("Resume 帧");
    assert_eq!(resume["op"], 6);
    assert_eq!(
        resume["d"]["token"], "QQBot tok-1",
        "RESUME 的 token 同样必须带 QQBot 前缀"
    );
    assert_eq!(resume["d"]["session_id"], "sess-1");
    assert_eq!(resume["d"]["seq"], 42);
    assert_eq!(resume["d"]["shard"], serde_json::json!([0, 1]));
    assert_eq!(auth_token("tok-1"), "QQBot tok-1");

    let heartbeat: serde_json::Value = serde_json::from_str(&heartbeat_frame(9)).expect("心跳帧");
    assert_eq!(heartbeat["op"], 1);
    assert_eq!(heartbeat["d"], 9);
}

#[test]
fn events_keep_raw_payload_and_survive_bad_frames() {
    let known = serde_json::json!({
        "op": 0,
        "s": 3,
        "t": "GROUP_AT_MESSAGE_CREATE",
        "id": "evt-3",
        "d": {"content": "你好"}
    })
    .to_string();
    let event = ProtocolEvent::from_text(&known).expect("分发事件");
    assert_eq!(event.kind(), Some(EventKind::GroupAtMessageCreate));
    assert!(event.is_known());
    assert_eq!(event.data["content"], "你好");

    let unknown =
        serde_json::json!({"op": 0, "s": 4, "t": "FUTURE_EVENT", "d": {"x": 1}}).to_string();
    let event = ProtocolEvent::from_text(&unknown).expect("未知事件也要透传");
    assert_eq!(event.kind(), None);
    assert!(!event.is_known());
    assert_eq!(event.data["x"], 1, "未知事件必须保留原始 d");
    assert!(event.raw.contains("FUTURE_EVENT"));

    let tolerant =
        serde_json::json!({"op": 0, "s": "坏值", "t": "READY", "d": {"session_id": "s-1"}})
            .to_string();
    let event = ProtocolEvent::from_text(&tolerant).expect("字段类型异常也要保留事件");
    assert_eq!(event.seq, None);
    assert_eq!(event.event_type.as_deref(), Some("READY"));
    assert_eq!(event.data["session_id"], "s-1", "解析异常不能丢 d");

    assert!(ProtocolEvent::from_text("不是 json").is_none());

    let ready = parse_ready(
        &serde_json::json!({
            "op": 0,
            "t": "READY",
            "d": {"session_id": "s-2", "user": {"id": "b-2", "username": "小助手"}}
        })
        .to_string(),
    );
    assert_eq!(ready.session_id, "s-2");
    assert_eq!(ready.bot_id, "b-2");
    assert_eq!(ready.bot_name, "小助手");
}

#[test]
fn states_and_stats_are_observable() {
    assert_eq!(GatewayState::default(), GatewayState::Disconnected);
    assert_eq!(GatewayState::Ready.as_str(), "ready");
    assert_eq!(GatewayState::Resuming.as_str(), "resuming");
    assert!(GatewayState::Ready.is_ready());
    assert!(!GatewayState::Connecting.is_ready());
    assert!(GatewayState::Stopped.is_terminal());
    assert!(!GatewayState::Ready.is_terminal());
    let stats = GatewayStats::default();
    assert_eq!(stats.shard, [0, 1]);
    assert_eq!(stats.seq, 0);
    assert!(!stats.ready);
    assert_eq!(stats.reconnects, 0);
}
