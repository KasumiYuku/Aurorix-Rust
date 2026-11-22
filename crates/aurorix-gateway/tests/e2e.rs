mod common;

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use aurorix_gateway::{
    GatewayClient, GatewayConfig, GatewayState, StaticTokenSource, DEFAULT_INTENT_BITS,
};
use common::{wait_state, RecordingSink};
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite;
use tungstenite::protocol::Message;

fn text(value: serde_json::Value) -> Message {
    Message::Text(value.to_string().into())
}

fn spawn_server(listener: TcpListener, seen: UnboundedSender<serde_json::Value>) {
    std::thread::spawn(move || {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(_) => return,
        };
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let mut socket = match tungstenite::accept(stream) {
            Ok(socket) => socket,
            Err(_) => return,
        };
        let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 30_000}});
        if socket.send(text(hello)).is_err() {
            return;
        }
        match socket.read() {
            Ok(Message::Text(payload)) => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) {
                    let _ = seen.send(value);
                }
            }
            _ => return,
        }
        let ready = serde_json::json!({
            "op": 0,
            "s": 1,
            "t": "READY",
            "d": {"session_id": "sess-e2e", "user": {"id": "bot-9", "username": "端到端机器人"}}
        });
        if socket.send(text(ready)).is_err() {
            return;
        }
        let event = serde_json::json!({
            "op": 0,
            "s": 2,
            "t": "GROUP_AT_MESSAGE_CREATE",
            "id": "evt-1",
            "d": {
                "id": "m-1",
                "content": "/help",
                "group_openid": "g-1",
                "author": {"member_openid": "u-1"}
            }
        });
        if socket.send(text(event)).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(400));
        let _ = socket.close(None);
    });
}

#[tokio::test]
async fn real_websocket_server_end_to_end() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
    let addr = listener.local_addr().expect("读取本地地址");
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    spawn_server(listener, sender);

    let tokens = Arc::new(StaticTokenSource::new("tok-123"));
    let sink = RecordingSink::new();
    let config = GatewayConfig::new(format!("ws://{addr}/"), DEFAULT_INTENT_BITS);
    let client = Arc::new(GatewayClient::new(config, tokens, sink.clone()));

    client.start();
    assert!(
        wait_state(&client, GatewayState::Ready, Duration::from_secs(5)).await,
        "真实服务端下必须握手成功并进入 Ready"
    );

    let identify = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("服务端应当收到 Identify")
        .expect("通道不应关闭");
    assert_eq!(identify["op"], 2, "客户端第一条必须是 Identify");
    assert_eq!(
        identify["d"]["token"], "QQBot tok-123",
        "token 必须带 QQBot 前缀"
    );
    assert_eq!(
        identify["d"]["intents"],
        serde_json::json!(DEFAULT_INTENT_BITS),
        "intents 位必须正确"
    );
    assert_eq!(identify["d"]["shard"], serde_json::json!([0, 1]));

    assert!(
        sink.wait_for(1, Duration::from_secs(5)).await,
        "真实服务端下发的事件必须进总线"
    );
    let events = sink.events();
    assert_eq!(
        events[0].event_type.as_deref(),
        Some("GROUP_AT_MESSAGE_CREATE")
    );
    assert_eq!(events[0].data["content"], "/help");
    assert_eq!(events[0].data["group_openid"], "g-1");
    assert_eq!(client.stats().session_id.as_deref(), Some("sess-e2e"));
    assert_eq!(client.stats().bot_name.as_deref(), Some("端到端机器人"));
    assert_eq!(client.stats().heartbeat_interval_ms, Some(30_000));

    client.shutdown().await;
    assert_eq!(client.stats().state, GatewayState::Stopped);
    assert_eq!(client.live_tasks(), 0, "停机后不能留下任务");
}
