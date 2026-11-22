use aurorix_gateway::{ConnectRequest, Transport, WebSocketTransport};

#[tokio::test]
#[ignore = "需要外网: 校验 wss 与真实网关的 TLS 握手"]
async fn wss_handshake_reaches_real_gateway() {
    let transport = WebSocketTransport::new();
    let outcome = transport
        .connect(ConnectRequest::new("wss://api.sgroup.qq.com/websocket"))
        .await;
    assert!(outcome.is_ok(), "wss 握手失败: {:?}", outcome.err());
}
