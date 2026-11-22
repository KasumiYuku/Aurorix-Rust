mod common;

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use aurorix_gateway::{
    GatewayClient, GatewayConfig, GatewayState, StaticTokenSource, DEFAULT_INTENT_BITS,
};
use aurorix_log::{Level, Logger, Options};
use common::{wait_state, RecordingSink};
use tokio_tungstenite::tungstenite;
use tungstenite::protocol::Message;

fn text(value: serde_json::Value) -> Message {
    Message::Text(value.to_string().into())
}

fn spawn_server(listener: TcpListener) {
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
        let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 12_345}});
        if socket.send(text(hello)).is_err() {
            return;
        }
        if !matches!(socket.read(), Ok(Message::Text(_))) {
            return;
        }
        let ready = serde_json::json!({
            "op": 0,
            "s": 1,
            "t": "READY",
            "d": {"session_id": "sess-log-123456", "user": {"id": "bot-1", "username": "日志机器人"}}
        });
        if socket.send(text(ready)).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(300));
        let _ = socket.close(None);
    });
}

#[tokio::test]
async fn lifecycle_shows_up_in_the_log() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
    let addr = listener.local_addr().expect("读取本地地址");
    spawn_server(listener);

    let logger = Logger::new(
        Options::default()
            .with_level(Level::Debug)
            .with_console(false),
    );
    let client = Arc::new(
        GatewayClient::new(
            GatewayConfig::new(format!("ws://{addr}/"), DEFAULT_INTENT_BITS),
            Arc::new(StaticTokenSource::new("tok-log")),
            RecordingSink::new(),
        )
        .with_logger(Arc::clone(&logger)),
    );

    client.start();
    assert!(
        wait_state(&client, GatewayState::Ready, Duration::from_secs(5)).await,
        "本地服务端下必须握手成功"
    );
    client.shutdown().await;

    let messages: Vec<String> = logger
        .recent(128)
        .into_iter()
        .map(|record| record.msg)
        .collect();
    let joined = messages.join("\n");
    for expected in [
        "网关状态 connecting → identified",
        "WebSocket 已连接, 等待 HELLO",
        "收到 HELLO 心跳间隔=12345ms",
        "发送 IDENTIFY intents=",
        "收到 READY bot=日志机器人",
        "网关状态 identified → ready",
    ] {
        assert!(
            joined.contains(expected),
            "日志里应当出现 {expected}, 实际:\n{joined}"
        );
    }
    assert!(
        messages.iter().any(|msg| msg.contains("session=sess-log…")),
        "READY 日志里的 session 必须截断"
    );
}
