mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aurorix_gateway::{
    callback_signature, ensure_webhook_compatible, signing_key, signing_seed, verify_signature,
    webhook_router, WebhookConfig, WebhookProcessor,
};
use common::RecordingSink;
use ed25519_dalek::Signer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SECRET: &str = "dg_test_app_secret";

fn sign(secret: &str, timestamp: &str, body: &[u8]) -> String {
    let mut message = Vec::new();
    message.extend_from_slice(timestamp.as_bytes());
    message.extend_from_slice(body);
    hex::encode(signing_key(secret).sign(&message).to_bytes())
}

#[test]
fn signing_seed_matches_go_doubling() {
    let seed = signing_seed("abc");
    assert_eq!(&seed[..6], b"abcabc");
    assert_eq!(
        &seed[..],
        b"abcabcabcabcabcabcabcabcabcabcab",
        "secret 必须自拼到 32 字节当 seed"
    );
    let long = signing_seed("0123456789012345678901234567890123456789");
    assert_eq!(
        &long[..],
        b"01234567890123456789012345678901",
        "超长 secret 只取前 32 字节"
    );
    assert_eq!(signing_seed(""), [0u8; 32], "空 secret 退化为全零 seed");
}

#[test]
fn signature_verification_accepts_and_rejects() {
    let body = br#"{"op":0,"t":"GROUP_AT_MESSAGE_CREATE","d":{}}"#;
    let timestamp = "1700000000";
    let signature = sign(SECRET, timestamp, body);
    assert!(
        verify_signature(SECRET, Some(&signature), Some(timestamp), body).is_ok(),
        "正确签名必须通过"
    );
    assert!(
        verify_signature(SECRET, Some(&signature), Some(timestamp), b"{}").is_err(),
        "换掉 body 必须验签失败"
    );
    assert!(
        verify_signature("别的密钥", Some(&signature), Some(timestamp), body).is_err(),
        "换掉密钥必须验签失败"
    );
    assert!(
        verify_signature(SECRET, None, Some(timestamp), body).is_err(),
        "缺签名头必须拒绝"
    );
    assert!(
        verify_signature(SECRET, Some(&signature), None, body).is_err(),
        "缺时间戳头必须拒绝"
    );
    assert!(
        verify_signature(SECRET, Some("zzzz"), Some(timestamp), body).is_err(),
        "非 hex 签名必须拒绝"
    );
    assert!(
        verify_signature(SECRET, Some("abcd"), Some(timestamp), body).is_err(),
        "长度不是 64 字节必须拒绝"
    );
}

#[test]
fn callback_validation_signature_matches_go_algorithm() {
    let event_ts = "1725442341";
    let plain_token = "token-abc";
    let expected = {
        let mut message = String::from(event_ts);
        message.push_str(plain_token);
        hex::encode(signing_key(SECRET).sign(message.as_bytes()).to_bytes())
    };
    assert_eq!(callback_signature(SECRET, event_ts, plain_token), expected);
    assert_ne!(
        callback_signature(SECRET, event_ts, plain_token),
        callback_signature(SECRET, plain_token, event_ts),
        "拼接顺序不能反"
    );

    let sink = RecordingSink::new();
    let processor = WebhookProcessor::new(SECRET, sink.clone());
    let body = serde_json::json!({
        "op": 13,
        "d": {"plain_token": plain_token, "event_ts": event_ts}
    })
    .to_string();
    let timestamp = "1700000000";
    let signature = sign(SECRET, timestamp, body.as_bytes());
    let outcome = processor.handle(Some(&signature), Some(timestamp), body.as_bytes());
    assert_eq!(outcome.reply.status, 200);
    let reply = outcome.reply.body.expect("op13 必须回包");
    assert_eq!(reply["plain_token"], plain_token);
    assert_eq!(reply["signature"], expected, "回包签名必须与 Go 算法一致");
    assert!(outcome.event.is_none(), "op13 不是业务事件");
}

#[test]
fn callback_validation_keeps_raw_numeric_event_ts() {
    let sink = RecordingSink::new();
    let processor = WebhookProcessor::new(SECRET, sink.clone());
    let body = serde_json::json!({
        "op": 13,
        "d": {"plain_token": "tok", "event_ts": 1725442341}
    })
    .to_string();
    let timestamp = "1700000000";
    let signature = sign(SECRET, timestamp, body.as_bytes());
    let outcome = processor.handle(Some(&signature), Some(timestamp), body.as_bytes());
    let reply = outcome.reply.body.expect("op13 必须回包");
    assert_eq!(
        reply["signature"],
        callback_signature(SECRET, "1725442341", "tok"),
        "数字型 event_ts 必须按原文参与签名"
    );
}

#[test]
fn webhook_rejects_guild_subscriptions() {
    let names = vec!["MESSAGE_CREATE".to_string()];
    let error = ensure_webhook_compatible(&names).expect_err("频道事件必须启动期拒绝");
    assert!(
        error.to_string().contains("频道"),
        "错误信息要说明是频道事件"
    );
    let config = WebhookConfig::new(SECRET, names);
    assert!(config.validate().is_err());
    let ok = WebhookConfig::new(SECRET, vec!["GROUP_AT_MESSAGE_CREATE".to_string()]);
    assert!(ok.validate().is_ok());
}

#[test]
fn unsigned_payload_is_rejected_before_parsing() {
    let sink = RecordingSink::new();
    let processor = WebhookProcessor::new(SECRET, sink.clone());
    let body = br#"{"op":0,"t":"GROUP_AT_MESSAGE_CREATE","d":{}}"#;
    let outcome = processor.handle(None, Some("1700000000"), body);
    assert_eq!(outcome.reply.status, 401);
    assert!(outcome.reply.body.is_none(), "401 不回包");
    assert!(outcome.event.is_none());
    assert_eq!(sink.len(), 0, "验签失败不能进业务总线");
}

async fn http_post(addr: SocketAddr, headers: Vec<(&str, String)>, body: &str) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("连接 webhook 服务");
    let mut request = format!(
        "POST /webhook HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).await.expect("写请求");
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await.expect("读响应");
    let text = String::from_utf8_lossy(&buffer).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    let body = text
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    (status, body)
}

#[tokio::test]
async fn router_accepts_signed_events_and_rejects_forgeries() {
    let sink = RecordingSink::new();
    let processor = Arc::new(WebhookProcessor::new(SECRET, sink.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定端口");
    let addr = listener.local_addr().expect("读取本地地址");
    let router = webhook_router(Arc::clone(&processor));
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let body = serde_json::json!({
        "op": 0,
        "s": 1,
        "id": "evt-1",
        "t": "GROUP_AT_MESSAGE_CREATE",
        "d": {"id": "m-1", "content": "嗨", "group_openid": "g-1"}
    })
    .to_string();
    let timestamp = "1700000000";
    let signature = sign(SECRET, timestamp, body.as_bytes());
    let good = vec![
        ("X-Signature-Ed25519", signature.clone()),
        ("X-Signature-Timestamp", timestamp.to_string()),
        ("Content-Type", "application/json".to_string()),
    ];
    let (status, _) = http_post(addr, good, &body).await;
    assert_eq!(status, 200, "验签通过必须回 200");
    assert!(
        sink.wait_for(1, Duration::from_secs(2)).await,
        "先回 200 再处理, 事件也要进总线"
    );
    let events = sink.events();
    assert_eq!(
        events[0].event_type.as_deref(),
        Some("GROUP_AT_MESSAGE_CREATE")
    );
    assert_eq!(events[0].data["content"], "嗨");

    let forged = vec![
        ("X-Signature-Ed25519", "00".repeat(64)),
        ("X-Signature-Timestamp", timestamp.to_string()),
    ];
    let (status, _) = http_post(addr, forged, &body).await;
    assert_eq!(status, 401, "伪造签名必须 401");

    let (status, _) = http_post(addr, Vec::new(), &body).await;
    assert_eq!(status, 401, "缺签名头必须 401");
    assert_eq!(sink.len(), 1, "被拒的请求不能进总线");
}

#[tokio::test]
async fn router_answers_callback_validation() {
    let sink = RecordingSink::new();
    let processor = Arc::new(WebhookProcessor::new(SECRET, sink.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定端口");
    let addr = listener.local_addr().expect("读取本地地址");
    let router = webhook_router(Arc::clone(&processor));
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let body = serde_json::json!({
        "op": 13,
        "d": {"plain_token": "token-abc", "event_ts": "1725442341"}
    })
    .to_string();
    let timestamp = "1700000000";
    let signature = sign(SECRET, timestamp, body.as_bytes());
    let headers = vec![
        ("X-Signature-Ed25519", signature),
        ("X-Signature-Timestamp", timestamp.to_string()),
    ];
    let (status, reply) = http_post(addr, headers, &body).await;
    assert_eq!(status, 200);
    let parsed: serde_json::Value = serde_json::from_str(reply.trim()).expect("回包必须是 JSON");
    assert_eq!(parsed["plain_token"], "token-abc");
    assert_eq!(
        parsed["signature"],
        callback_signature(SECRET, "1725442341", "token-abc"),
        "回包签名必须与 Go 算法一致"
    );
}
