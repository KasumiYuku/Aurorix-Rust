mod support;

use aurorix_api::{
    Anchor, BotApi, MessageKind, Outbound, Scene, SendRequest, SeqBudget, Target, GUILD_TEXT_LIMIT,
};
use serde_json::Value;
use std::sync::Arc;
use support::{MockResponse, MockServer};

fn body_of(server: &MockServer, index: usize) -> Value {
    server.api_requests()[index].json()
}

#[tokio::test]
async fn four_scenes_use_their_own_paths_and_shape() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let cases = [
        (Scene::Group, "g1", "/v2/groups/g1/messages"),
        (Scene::C2c, "u1", "/v2/users/u1/messages"),
        (Scene::Channel, "c1", "/channels/c1/messages"),
        (Scene::Dm, "gu1", "/dms/gu1/messages"),
    ];
    for (scene, id, _) in cases {
        api.send(&Target::new(scene, id), &Outbound::text("你好"))
            .await
            .expect("发送要成功");
    }
    let requests = server.api_requests();
    assert_eq!(requests.len(), 4, "四条路径各一次");
    for (request, (_, _, expected)) in requests.iter().zip(cases.iter()) {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, *expected, "路径要按场景分派");
        assert_eq!(request.header("authorization"), "QQBot tok-1");
        assert!(request
            .header("content-type")
            .starts_with("application/json"));
    }
    let parsed: SendRequest =
        serde_json::from_slice(&requests[0].body).expect("报文要是 SendRequest");
    assert_eq!(parsed.msg_type, MessageKind::PlainText.code());
    assert_eq!(parsed.content, "你好");
    assert_eq!(parsed.msg_id, None);
}

#[tokio::test]
async fn msg_seq_counts_up_from_one() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let budget = SeqBudget::shared();
    let target = Target::new(Scene::Group, "g1");
    for _ in 0..3 {
        let outbound = Outbound::text("x")
            .with_msg_id("m1")
            .with_budget(Arc::clone(&budget));
        api.send(&target, &outbound).await.expect("发送要成功");
    }
    let seqs: Vec<u64> = (0..3)
        .map(|index| {
            body_of(&server, index)["msg_seq"]
                .as_u64()
                .expect("要有 msg_seq")
        })
        .collect();
    assert_eq!(seqs, vec![1, 2, 3], "msg_seq 要原子自增且从 1 起");
    assert!(!budget.spent());
    assert_eq!(budget.sent(), 3);
}

#[tokio::test]
async fn initiative_message_has_no_anchor_fields() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let outbound = Outbound::text("主动推送")
        .with_anchor(Anchor::new("m1", "e1"))
        .with_budget(SeqBudget::shared())
        .as_initiative();
    api.send(&Target::new(Scene::Group, "g1"), &outbound)
        .await
        .expect("发送要成功");
    let body = body_of(&server, 0);
    assert!(body.get("msg_seq").is_none(), "主动消息不能带 msg_seq");
    assert!(body.get("msg_id").is_none(), "主动消息不能带 msg_id");
    assert!(body.get("event_id").is_none(), "主动消息不能带 event_id");
    assert_eq!(body["msg_type"], 0);
}

#[tokio::test]
async fn channel_keeps_msg_id_only() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let outbound = Outbound::markdown("# 标题\n正文")
        .with_anchor(Anchor::new("m1", "e1"))
        .with_budget(SeqBudget::shared());
    api.send(&Target::new(Scene::Channel, "c1"), &outbound)
        .await
        .expect("发送要成功");
    let body = body_of(&server, 0);
    assert_eq!(body["msg_id"], "m1", "频道必须带 msg_id");
    assert!(body.get("event_id").is_none(), "频道不带 event_id");
    assert!(body.get("msg_seq").is_none(), "频道不带 msg_seq");
    assert_eq!(body["msg_type"], 0, "频道只能发纯文本");
    assert!(body.get("markdown").is_none(), "频道没有 markdown 通道");
    assert_eq!(body["content"], "标题\n正文");
}

#[tokio::test]
async fn channel_filters_urls_and_splits_images() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let content = "# 标题\n看 https://evil.example/x\n![](https://img.example/a.png)\n[锚点](https://link.example/y)";
    let outbound = Outbound::markdown(content)
        .with_anchor(Anchor::new("m1", "e1"))
        .with_budget(SeqBudget::shared());
    api.send(&Target::new(Scene::Channel, "c1"), &outbound)
        .await
        .expect("发送要成功");
    let requests = server.api_requests();
    assert_eq!(requests.len(), 2, "正文一条 + 图片单独一条");
    let text = requests[0].json();
    assert_eq!(text["content"], "标题\n看\n锚点", "链接留锚文本、裸链抹掉");
    assert!(
        !text["content"].as_str().unwrap().contains("http"),
        "正文不能留 URL"
    );
    let image = requests[1].json();
    assert_eq!(image["image"], "https://img.example/a.png");
    assert_eq!(
        image["msg_id"], "m1",
        "频道图片也要带 msg_id，否则变主动消息"
    );
}

#[tokio::test]
async fn channel_body_is_truncated_to_two_thousand_runes() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let long = "汉".repeat(2500);
    api.send(&Target::new(Scene::Channel, "c1"), &Outbound::text(long))
        .await
        .expect("发送要成功");
    let content = body_of(&server, 0)["content"].as_str().unwrap().to_string();
    assert_eq!(
        content.chars().count(),
        GUILD_TEXT_LIMIT,
        "截断后正好 2000 rune"
    );
    assert!(content.ends_with('…'), "截断要补省略号");
}

#[tokio::test]
async fn empty_body_counts_as_success() {
    let server = MockServer::with_script(vec![MockResponse::empty()]).await;
    let outcome = server
        .api()
        .send(&Target::new(Scene::Group, "g1"), &Outbound::text("x"))
        .await
        .expect("空体也算成功");
    assert_eq!(outcome.message_id, None, "没有 id 不算失败");
    assert_eq!(server.api_count(), 1, "空体不该触发重试");
}

#[tokio::test]
async fn body_without_id_counts_as_success() {
    let server =
        MockServer::with_script(vec![MockResponse::json(r#"{"timestamp":"1700000000"}"#)]).await;
    let outcome = server
        .api()
        .send(&Target::new(Scene::Group, "g1"), &Outbound::text("x"))
        .await
        .expect("缺 id 也算成功");
    assert_eq!(outcome.message_id, None);
    assert_eq!(outcome.audit_id, None);
    assert_eq!(server.api_count(), 1);
}

#[tokio::test]
async fn group_markdown_keeps_markdown_body() {
    let server = MockServer::with_script(Vec::new()).await;
    let api: BotApi = server.api();
    api.send(
        &Target::new(Scene::Group, "g1"),
        &Outbound::markdown("**粗体**"),
    )
    .await
    .expect("发送要成功");
    let body = body_of(&server, 0);
    assert_eq!(body["msg_type"], 2);
    assert_eq!(body["markdown"]["content"], "**粗体**");
    assert!(
        body.get("content").is_none(),
        "markdown 报文与平台一致, 不发空 content"
    );
}

#[tokio::test]
async fn media_message_carries_file_info() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    api.send(
        &Target::new(Scene::Group, "g1"),
        &Outbound::media("", serde_json::json!({"file_info": "fi-1"})),
    )
    .await
    .expect("发送要成功");
    let body = body_of(&server, 0);
    assert_eq!(body["msg_type"], 7);
    assert_eq!(body["media"]["file_info"], "fi-1");
}

#[tokio::test]
async fn guild_media_is_rejected_locally() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let err = api
        .send(
            &Target::new(Scene::Channel, "c1"),
            &Outbound::media("", serde_json::json!({"file_info": "fi-1"})),
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("不支持"),
        "频道富媒体要本地拒绝: {err}"
    );
    assert_eq!(server.api_count(), 0, "本地拒绝不该发请求");
}

#[tokio::test]
async fn empty_target_is_rejected() {
    let server = MockServer::with_script(Vec::new()).await;
    let err = server
        .api()
        .send(&Target::new(Scene::Group, ""), &Outbound::text("x"))
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "会话目标为空");
    assert_eq!(server.api_count(), 0);
}

#[tokio::test]
async fn quote_reference_rides_along_with_passive_send() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let outbound = Outbound::text("引用回复")
        .with_msg_id("m-1")
        .quote_to("m-orig");
    api.send(&Target::new(Scene::Group, "g1"), &outbound)
        .await
        .expect("发送要成功");
    let body = body_of(&server, 0);
    assert_eq!(body["msg_id"], serde_json::json!("m-1"));
    assert_eq!(
        body["message_reference"]["message_id"],
        serde_json::json!("m-orig"),
        "被动回复要带 message_reference"
    );
    assert_eq!(
        body["message_reference"]["ignore_get_message_error"],
        serde_json::json!(false),
        "Go 的结构体没有 omitempty, 这个键必须出现"
    );
}

#[tokio::test]
async fn quote_reference_is_dropped_on_initiative_send() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let mut outbound = Outbound::text("主动推送").quote_to("m-orig");
    outbound.initiative = true;
    api.send(&Target::new(Scene::Group, "g1"), &outbound)
        .await
        .expect("发送要成功");
    let body = body_of(&server, 0);
    assert!(
        body.get("message_reference").is_none(),
        "主动推送没有原消息可引用, 必须清掉: {body}"
    );
    assert!(body.get("msg_id").is_none(), "主动推送不能带 msg_id");
}

#[tokio::test]
async fn channel_image_bytes_are_posted_as_multipart() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let png = b"\x89PNG\r\n\x1a\nraw-bytes".to_vec();
    let id = api
        .send_channel_image_bytes(
            &Target::new(Scene::Channel, "c1"),
            "看图",
            Some("m-1"),
            "a.png",
            "image/png",
            &png,
        )
        .await
        .expect("字节直传要成功");
    assert!(id.is_some(), "要拿到消息 id");
    let request = server.last();
    assert_eq!(request.path, "/channels/c1/messages");
    assert!(
        request
            .header("content-type")
            .starts_with("multipart/form-data"),
        "字节直传要用 multipart: {}",
        request.header("content-type")
    );
    let raw = String::from_utf8_lossy(&request.body).to_string();
    assert!(raw.contains("name=\"file_image\""), "字段名要是 file_image");
    assert!(raw.contains("filename=\"a.png\""), "要带文件名");
    assert!(raw.contains("name=\"content\""), "要带 content 字段");
    assert!(raw.contains("看图"), "content 内容要原样带上");
    assert!(raw.contains("name=\"msg_id\""), "被动发图要带 msg_id");
}

#[tokio::test]
async fn channel_image_bytes_reject_non_guild_scene() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let err = api
        .send_channel_image_bytes(
            &Target::new(Scene::Group, "g1"),
            "",
            None,
            "a.png",
            "image/png",
            b"png",
        )
        .await
        .expect_err("群聊不支持频道字节发图");
    assert_eq!(server.api_count(), 0, "被拒绝时不该发请求");
    assert!(err.to_string().contains("频道图片"), "{err}");
}
