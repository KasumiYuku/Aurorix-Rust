mod common;

use std::sync::Arc;
use std::time::Duration;

use aurorix_gateway::{Frame, GatewayState, DEFAULT_INTENT_BITS};
use common::*;
use serde_json::json;

const WAIT: Duration = Duration::from_millis(3_000);

#[tokio::test]
async fn identify_uses_qqbot_token_and_intent_bits() {
    let transport = CountingTransport::fixed(
        ConnScript::hello(30_000)
            .with_identify(vec![ready("sess-1", "bot-1", "小助手", 1)])
            .with_heartbeat(vec![ack()]),
    );
    let (client, _tokens, sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-123",
        transport.clone(),
    );

    client.start();
    assert!(
        wait_state(&client, GatewayState::Ready, WAIT).await,
        "收到 READY 后必须进入 Ready"
    );

    let sent = transport.socket(0).sent_json();
    assert_eq!(sent.len(), 1, "首条只应有 Identify");
    assert_eq!(sent[0]["op"], 2, "首条必须是 op=2 Identify");
    assert_eq!(
        sent[0]["d"]["token"], "QQBot tok-123",
        "token 必须带 QQBot 前缀"
    );
    assert_eq!(
        sent[0]["d"]["intents"],
        json!(DEFAULT_INTENT_BITS),
        "intents 位必须与对账单一致"
    );
    assert_eq!(sent[0]["d"]["shard"], json!([0, 1]), "shard 固定 [0, 1]");

    let stats = client.stats();
    assert_eq!(stats.session_id.as_deref(), Some("sess-1"));
    assert_eq!(stats.bot_id.as_deref(), Some("bot-1"));
    assert_eq!(stats.bot_name.as_deref(), Some("小助手"));
    assert!(stats.ready);
    assert_eq!(stats.heartbeat_interval_ms, Some(30_000));
    assert_eq!(sink.len(), 0, "READY 不能投给业务总线");

    client.shutdown().await;
    assert_eq!(client.stats().state, GatewayState::Stopped);
}

#[tokio::test]
async fn dispatch_and_unknown_events_reach_the_sink() {
    let transport = CountingTransport::fixed(
        ConnScript::hello(30_000)
            .with_identify(vec![ready("sess-2", "bot-2", "小助手", 1)])
            .with_heartbeat(vec![ack()]),
    );
    let (client, _tokens, sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-1",
        transport.clone(),
    );

    client.start();
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);

    let socket = transport.socket(0);
    socket.push(group_message(2, "/help"));
    socket.push(dispatch("FUTURE_EVENT", 3, json!({"x": 1, "y": "原样"})));

    assert!(sink.wait_for(2, WAIT).await, "两个事件都要进总线");
    let events = sink.events();
    assert_eq!(
        events[0].kind(),
        Some(aurorix_protocol::EventKind::GroupAtMessageCreate)
    );
    assert_eq!(events[0].data["content"], "/help");
    assert_eq!(events[0].seq, Some(2));
    assert_eq!(events[1].kind(), None, "未知事件名解析不到 kind");
    assert_eq!(events[1].event_type.as_deref(), Some("FUTURE_EVENT"));
    assert_eq!(events[1].data["y"], "原样", "未知事件必须保留原始 d");
    assert_eq!(client.stats().seq, 3);
    assert!(client.stats().last_event_at.is_some());

    client.shutdown().await;
}

#[tokio::test]
async fn resume_carries_session_id_and_seq() {
    let transport = CountingTransport::new(|index| {
        let _ = index;
        ConnScript::hello(30_000)
            .with_heartbeat(vec![ack()])
            .with_identify(vec![ready("sess-9", "bot-9", "小助手", 1)])
            .with_resume(vec![resumed(21)])
    });
    let (client, _tokens, _sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-777",
        transport.clone(),
    );

    client.start();
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);
    let first = transport.socket(0);
    first.push(group_message(20, "先来一条"));
    assert!(
        wait_until(|| client.seq() == 20, WAIT).await,
        "seq 应推进到 20"
    );
    first.push(Frame::Close(1006));
    assert!(
        wait_until(|| transport.connects() >= 2, WAIT).await,
        "断线后必须重连"
    );
    assert!(
        wait_state(&client, GatewayState::Ready, WAIT).await,
        "RESUMED 之后必须回到 Ready"
    );

    let second = transport.socket(1).sent_json();
    assert_eq!(second[0]["op"], 6, "有 session 时必须发 Resume");
    assert_eq!(
        second[0]["d"]["token"], "QQBot tok-777",
        "Resume 的 token 必须带 QQBot 前缀"
    );
    assert_eq!(second[0]["d"]["session_id"], "sess-9");
    assert_eq!(second[0]["d"]["seq"], 20, "Resume 要带上当前 seq");
    assert_eq!(second[0]["d"]["shard"], serde_json::json!([0, 1]));

    client.shutdown().await;
}

#[tokio::test]
async fn close_4914_stops_reconnecting() {
    let transport =
        CountingTransport::fixed(ConnScript::hello(30_000).with_identify(vec![Frame::Close(4914)]));
    let config = fast_config("ws://gateway.test/")
        .with_backoff(Duration::from_millis(20), Duration::from_millis(40));
    let (client, _tokens, _sink) = build(config, "tok-1", transport.clone());

    client.start();
    assert!(
        wait_state(&client, GatewayState::Stopped, WAIT).await,
        "4914 必须进入 Stopped"
    );
    assert_eq!(client.stats().last_close_code, Some(4914));
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(transport.connects(), 1, "Stopped 之后不能再建连接");
}

#[tokio::test]
async fn close_4915_stops_reconnecting() {
    let transport =
        CountingTransport::fixed(ConnScript::hello(30_000).with_identify(vec![Frame::Close(4915)]));
    let (client, _tokens, _sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-1",
        transport.clone(),
    );

    client.start();
    assert!(
        wait_state(&client, GatewayState::Stopped, WAIT).await,
        "4915 必须进入 Stopped"
    );
    assert_eq!(client.stats().last_close_code, Some(4915));
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(transport.connects(), 1, "Stopped 之后不能再建连接");
}

#[tokio::test]
async fn close_4004_invalidates_token_then_identifies() {
    let transport = CountingTransport::new(|index| {
        if index == 0 {
            ConnScript::hello(30_000).with_identify(vec![Frame::Close(4004)])
        } else {
            ConnScript::hello(30_000)
                .with_identify(vec![ready("sess-new", "bot-1", "小助手", 1)])
                .with_heartbeat(vec![ack()])
        }
    });
    let (client, tokens, _sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-1",
        transport.clone(),
    );

    client.start();
    assert!(
        wait_until(|| transport.connects() >= 2, WAIT).await,
        "4004 之后必须重新连接"
    );
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);
    assert_eq!(tokens.invalidations(), 1, "4004 必须作废一次 token");
    let second = transport.socket(1).sent_json();
    assert_eq!(
        second[0]["op"], 2,
        "4004 之后必须重新 Identify 而不是 Resume"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn close_4006_clears_session_and_identifies_again() {
    let transport = CountingTransport::new(|index| {
        if index == 0 {
            ConnScript::hello(30_000)
                .with_identify(vec![ready("sess-old", "bot-1", "小助手", 5)])
                .with_heartbeat(vec![ack()])
        } else {
            ConnScript::hello(30_000)
                .with_identify(vec![ready("sess-old", "bot-1", "小助手", 1)])
                .with_heartbeat(vec![ack()])
        }
    });
    let (client, tokens, _sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-1",
        transport.clone(),
    );

    client.start();
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);
    assert_eq!(client.session_id().as_deref(), Some("sess-old"));
    transport.socket(0).push(Frame::Close(4006));
    assert!(
        wait_until(|| transport.connects() >= 2, WAIT).await,
        "4006 之后必须重新连接"
    );
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);
    assert_eq!(tokens.invalidations(), 0, "4006 不该作废 token");
    let second = transport.socket(1).sent_json();
    assert_eq!(second[0]["op"], 2, "4006 之后必须重新 Identify");

    client.shutdown().await;
}

#[tokio::test]
async fn heartbeat_without_ack_kills_the_connection() {
    let transport = CountingTransport::fixed(
        ConnScript::hello(40)
            .with_identify(vec![ready("sess-hb", "bot-1", "小助手", 1)])
            .with_heartbeat(vec![]),
    );
    let (client, _tokens, _sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-1",
        transport.clone(),
    );

    client.start();
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);
    assert!(
        wait_until(|| transport.connects() >= 2, WAIT).await,
        "心跳没有 ACK 必须掐线重连"
    );
    assert!(transport.socket(0).is_closed(), "旧连接必须被主动关闭");

    client.shutdown().await;
}

#[tokio::test]
async fn heartbeat_frames_carry_current_seq() {
    let transport = CountingTransport::fixed(
        ConnScript::hello(20)
            .with_identify(vec![ready("sess-hb2", "bot-1", "小助手", 11)])
            .with_heartbeat(vec![ack()]),
    );
    let (client, _tokens, _sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-1",
        transport.clone(),
    );

    client.start();
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);
    let socket = transport.socket(0);
    assert!(
        wait_until(
            || socket
                .sent_json()
                .iter()
                .any(|value| value["op"] == json!(1)),
            WAIT
        )
        .await,
        "就绪后必须开始发心跳"
    );
    let heartbeat = socket
        .sent_json()
        .into_iter()
        .find(|value| value["op"] == json!(1))
        .expect("应有心跳帧");
    assert_eq!(heartbeat["d"], 11, "心跳 d 必须带当前 seq");

    client.shutdown().await;
}

#[tokio::test]
async fn seq_ignores_regressions() {
    let transport = CountingTransport::fixed(
        ConnScript::hello(20)
            .with_identify(vec![ready("sess-seq", "bot-1", "小助手", 10)])
            .with_heartbeat(vec![ack()]),
    );
    let (client, _tokens, _sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-1",
        transport.clone(),
    );

    client.start();
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);
    assert_eq!(client.stats().seq, 10);
    let socket = transport.socket(0);
    socket.push(group_message(4, "更小的 seq"));
    assert!(wait_until(|| client.seq() == 10, WAIT).await);
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(client.stats().seq, 10, "seq 回退必须被忽略");
    socket.push(group_message(12, "更大的 seq"));
    assert!(
        wait_until(|| client.stats().seq == 12, WAIT).await,
        "seq 应继续前进"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn five_strikes_drop_the_session() {
    let transport = CountingTransport::new(|index| match index {
        0 => ConnScript::hello(30_000).with_identify(vec![
            ready("sess-drop", "bot-1", "小助手", 1),
            Frame::Close(1000),
        ]),
        1..=5 => ConnScript::hello(30_000).with_resume(vec![Frame::Close(1006)]),
        _ => ConnScript::hello(30_000).with_identify(vec![ready("sess-new", "bot-1", "小助手", 2)]),
    });
    let config = fast_config("ws://gateway.test/")
        .with_backoff(Duration::from_millis(5), Duration::from_millis(20))
        .with_reauth_delay(Duration::from_millis(5))
        .with_strikes(5);
    let (client, _tokens, _sink) = build(config, "tok-1", transport.clone());

    client.start();
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);
    assert!(
        wait_until(|| client.session_id().as_deref() == Some("sess-new"), WAIT).await,
        "连续 5 次未就绪必须丢弃 session 并重新 Identify"
    );

    let ops = transport.first_ops();
    assert!(ops.len() >= 7, "至少应有 7 次建连");
    assert_eq!(ops[0], 2, "首连走 Identify");
    for (index, op) in ops.iter().enumerate().take(6).skip(1) {
        assert_eq!(*op, 6, "第 {index} 次重连仍应带 session 走 Resume");
    }
    assert_eq!(ops[6], 2, "第 5 次 strike 之后必须重新 Identify");
    assert_eq!(client.seq(), 2, "重新鉴权后 seq 从新的 READY 起算");

    client.shutdown().await;
}

#[tokio::test]
async fn read_timeout_forces_reconnect() {
    let transport = CountingTransport::fixed(ConnScript::hello(0).with_identify(vec![ready(
        "sess-idle",
        "bot-1",
        "小助手",
        1,
    )]));
    let config = fast_config("ws://gateway.test/")
        .with_backoff(Duration::from_millis(5), Duration::from_millis(20))
        .with_read_window(Duration::from_millis(30));
    let (client, _tokens, _sink) = build(config, "tok-1", transport.clone());

    client.start();
    assert!(
        wait_until(|| transport.connects() >= 2, WAIT).await,
        "读超时必须触发重连"
    );
    assert_eq!(
        client.stats().heartbeat_interval_ms,
        Some(0),
        "间隔为 0 时不启动心跳"
    );

    client.shutdown().await;
}

#[tokio::test]
async fn shutdown_leaves_no_live_tasks() {
    let transport = CountingTransport::fixed(
        ConnScript::hello(20)
            .with_identify(vec![ready("sess-stop", "bot-1", "小助手", 1)])
            .with_heartbeat(vec![ack()]),
    );
    let (client, _tokens, _sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-1",
        transport.clone(),
    );

    client.start();
    assert!(wait_state(&client, GatewayState::Ready, WAIT).await);
    assert!(
        wait_until(|| client.live_tasks() >= 1, WAIT).await,
        "就绪后应有心跳任务在跑"
    );

    client.shutdown().await;
    assert!(client.is_shutdown(), "停机后停机标志仍需为真");
    assert_eq!(client.stats().state, GatewayState::Stopped);
    assert_eq!(client.live_tasks(), 0, "shutdown 后任务数必须归零");
    let connects = transport.connects();
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(transport.connects(), connects, "shutdown 后不能再建连接");
    assert_eq!(Arc::strong_count(&client), 1, "客户端不应残留内部引用");
}

#[tokio::test]
async fn lifecycle_dispatch_keeps_a_single_heartbeat() {
    let transport = CountingTransport::fixed(
        ConnScript::hello(60)
            .with_identify(vec![ready("sess-hb", "bot-1", "小助手", 1)])
            .with_heartbeat(vec![ack()]),
    );
    let (client, _tokens, _sink) = build(
        fast_config("ws://gateway.test/"),
        "tok-1",
        transport.clone(),
    );

    client.start();
    assert!(
        wait_until(|| client.ready_count() >= 1, WAIT).await,
        "收到 READY 后必须计入就绪次数"
    );
    let socket = transport.socket(0);
    for step in 0..8 {
        socket.push(resumed(step + 2));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        wait_until(|| client.ready_count() >= 9, WAIT).await,
        "RESUMED 也必须计入就绪次数"
    );
    assert_eq!(client.live_tasks(), 1, "同一连接只允许一个心跳任务");
    let beats = socket
        .sent_json()
        .iter()
        .filter(|value| value["op"] == 1)
        .count();
    assert!(beats >= 1, "生命周期帧不能推迟心跳");

    client.shutdown().await;
    assert_eq!(client.live_tasks(), 0, "停机后心跳任务必须退出");
}
