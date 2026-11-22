mod support;

use std::sync::Arc;
use std::time::Duration;

use aurorix_api::{
    Anchor, ApiError, Outbound, Scene, SeqBudget, Target, AUDIT_TIMEOUT, EVENT_ID_RETRY_CODE,
    MAX_PASSIVE_SEQ,
};
use support::{MockResponse, MockServer};

fn group_target() -> Target {
    Target::new(Scene::Group, "g1")
}

#[tokio::test]
async fn listed_retry_when_code_is_retried() {
    let server = MockServer::with_script(vec![
        MockResponse::platform(11253, "按钮回调权限不足"),
        MockResponse::ok_id("m1"),
    ])
    .await;
    let outcome = server
        .api()
        .send(&group_target(), &Outbound::text("x"))
        .await
        .expect("重试之后要成功");
    assert_eq!(server.api_count(), 2, "retry_when 命中的码要重试一次");
    assert_eq!(outcome.message_id.as_deref(), Some("m1"));
}

#[tokio::test]
async fn unlisted_unknown_code_is_not_retried() {
    let server = MockServer::with_script(vec![
        MockResponse::platform(424242, "谁也不知道"),
        MockResponse::ok_id("m1"),
    ])
    .await;
    let err = server
        .api()
        .send(&group_target(), &Outbound::text("x"))
        .await
        .unwrap_err();
    assert_eq!(server.api_count(), 1, "没列进 retry_when 的码只打一次");
    assert_eq!(err.code(), Some(424242));
}

#[tokio::test]
async fn server_side_code_is_retried_without_config() {
    let server = MockServer::with_script(vec![
        MockResponse::platform(50055001, "消息发送异常"),
        MockResponse::ok_id("m2"),
    ])
    .await;
    server
        .api()
        .send(&group_target(), &Outbound::text("x"))
        .await
        .expect("服务器类错误自动重试");
    assert_eq!(server.api_count(), 2);
}

#[tokio::test]
async fn event_id_not_ready_retries_three_times() {
    let server = MockServer::with_script(vec![
        MockResponse::platform(EVENT_ID_RETRY_CODE, "event_id 尚未落库"),
        MockResponse::platform(EVENT_ID_RETRY_CODE, "event_id 尚未落库"),
        MockResponse::platform(EVENT_ID_RETRY_CODE, "event_id 尚未落库"),
        MockResponse::platform(EVENT_ID_RETRY_CODE, "event_id 尚未落库"),
        MockResponse::ok_id("never"),
    ])
    .await;
    let err = server
        .api()
        .send(&group_target(), &Outbound::text("x"))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(EVENT_ID_RETRY_CODE));
    assert_eq!(server.api_count(), 4, "40034025 要重试 3 次共 4 次请求");
}

#[tokio::test]
async fn audit_pending_times_out() {
    assert_eq!(AUDIT_TIMEOUT, Duration::from_secs(30), "审核等待上限 30 秒");
    let server = MockServer::with_script(vec![MockResponse::json(
        r#"{"code":304023,"message":"消息正在审核中","data":{"message_audit":{"audit_id":"a1"}}}"#,
    )])
    .await;
    let api = server.api();
    api.set_audit_timeout(Duration::from_millis(120));
    let err = api
        .send(&group_target(), &Outbound::text("x"))
        .await
        .unwrap_err();
    assert_eq!(err, ApiError::AuditTimeout, "没人回审核结果就是超时失败");
    assert_eq!(server.api_count(), 1, "等审核期间不该重发");
}

#[tokio::test]
async fn audit_pass_releases_the_send() {
    let server = MockServer::with_script(vec![MockResponse::json(
        r#"{"code":304023,"message":"消息正在审核中","data":{"message_audit":{"audit_id":"a1"}}}"#,
    )])
    .await;
    let api = Arc::new(server.api());
    api.set_audit_timeout(Duration::from_millis(500));
    let resolver = {
        let clone = Arc::clone(&api);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            clone.resolve_audit("a1", "m9", true)
        })
    };
    let outcome = api
        .send(&group_target(), &Outbound::text("x"))
        .await
        .expect("审核通过后发送算成功");
    assert!(resolver.await.unwrap(), "事件层要找到等待者");
    assert_eq!(outcome.message_id, None);
    assert_eq!(server.api_count(), 1);
}

#[tokio::test]
async fn audit_reject_fails_the_send() {
    let server = MockServer::with_script(vec![MockResponse::json(
        r#"{"code":304023,"message":"消息正在审核中","data":{"message_audit":{"audit_id":"a9"}}}"#,
    )])
    .await;
    let api = Arc::new(server.api());
    api.set_audit_timeout(Duration::from_millis(500));
    let resolver = {
        let clone = Arc::clone(&api);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            clone.resolve_audit("a9", "", false)
        })
    };
    let err = api
        .send(&group_target(), &Outbound::text("x"))
        .await
        .unwrap_err();
    assert_eq!(err, ApiError::AuditRejected);
    assert!(resolver.await.unwrap());
}

#[tokio::test]
async fn passive_expiry_in_http_400_degrades_to_active() {
    let server = MockServer::with_script(vec![
        MockResponse::platform(40034031, "消息已过期，不能回复"),
        MockResponse::ok_id("m2"),
    ])
    .await;
    let api = server.api();
    let outbound = Outbound::text("x")
        .with_anchor(Anchor::new("m1", "e1"))
        .with_budget(SeqBudget::shared());
    let outcome = api
        .send(&group_target(), &outbound)
        .await
        .expect("要降级成功");
    assert_eq!(server.api_count(), 2);
    let first = server.api_requests()[0].json();
    assert_eq!(first["msg_seq"], 1);
    assert_eq!(first["msg_id"], "m1");
    assert_eq!(first["event_id"], "e1");
    let second = server.api_requests()[1].json();
    assert!(second.get("msg_seq").is_none(), "降级要清空 msg_seq");
    assert!(second.get("msg_id").is_none(), "降级要清空 msg_id");
    assert!(second.get("event_id").is_none(), "降级要清空 event_id");
    assert!(outcome.delivered_active, "降级后的这条算主动投递");
    assert!(outcome.fallback);
}

#[tokio::test]
async fn passive_expiry_in_http_200_degrades_to_active() {
    let server = MockServer::with_script(vec![
        MockResponse::json(r#"{"code":304103,"message":"消息 ID 已过期，不能回复"}"#),
        MockResponse::ok_id("m2"),
    ])
    .await;
    let api = server.api();
    let outbound = Outbound::text("x")
        .with_msg_id("m1")
        .with_budget(SeqBudget::shared());
    let outcome = api
        .send(&group_target(), &outbound)
        .await
        .expect("要降级成功");
    assert_eq!(server.api_count(), 2, "200 + 业务码也要降级");
    assert!(outcome.delivered_active);
}

#[tokio::test]
async fn duplicate_msg_seq_degrades_to_active() {
    let server = MockServer::with_script(vec![
        MockResponse::platform(40054005, "消息被去重"),
        MockResponse::ok_id("m2"),
    ])
    .await;
    let api = server.api();
    let outbound = Outbound::text("x")
        .with_msg_id("m1")
        .with_budget(SeqBudget::shared());
    let outcome = api
        .send(&group_target(), &outbound)
        .await
        .expect("重复 msg_seq 要降级为主动消息");
    assert_eq!(server.api_count(), 2, "被动失败后必须再走主动通道");
    let second = server.api_requests()[1].json();
    assert!(second.get("msg_seq").is_none());
    assert!(second.get("msg_id").is_none());
    assert!(outcome.delivered_active);
    assert!(outcome.fallback);
}

#[tokio::test]
async fn content_violation_never_degrades() {
    let server = MockServer::with_script(vec![
        MockResponse::platform(40034006, "消息内容违规"),
        MockResponse::ok_id("m2"),
    ])
    .await;
    let api = server.api();
    let outbound = Outbound::text("x")
        .with_msg_id("m1")
        .with_budget(SeqBudget::shared());
    let err = api.send(&group_target(), &outbound).await.unwrap_err();
    assert_eq!(err.code(), Some(40034006));
    assert_eq!(server.api_count(), 1, "内容违规换通道也发不出去");
}

#[tokio::test]
async fn exhausted_budget_falls_back_to_active_channel() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let budget = SeqBudget::shared();
    for _ in 0..MAX_PASSIVE_SEQ {
        budget.count().expect("前 5 条要用得掉");
    }
    let outbound = Outbound::text("x")
        .with_msg_id("m1")
        .with_budget(Arc::clone(&budget));
    let outcome = api
        .send(&group_target(), &outbound)
        .await
        .expect("第 6 条要转主动");
    assert_eq!(server.api_count(), 1, "被动层本地就拦掉，只剩主动一次");
    let body = server.last().json();
    assert!(body.get("msg_seq").is_none());
    assert!(body.get("msg_id").is_none());
    assert!(outcome.delivered_active);
}

#[tokio::test]
async fn group_falls_back_to_private_chat() {
    let server = MockServer::with_script(vec![
        MockResponse::platform(40034031, "消息已过期，不能回复"),
        MockResponse::platform(40034105, "主动消息发送失败，无权限"),
        MockResponse::ok_id("m3"),
    ])
    .await;
    let api = server.api();
    let target = Target::new(Scene::Group, "g1").with_user("u1");
    let outcome = api
        .send(&target, &Outbound::text("x").with_msg_id("m1"))
        .await
        .expect("第三层要能发出去");
    let paths: Vec<String> = server.paths();
    assert_eq!(
        paths,
        vec![
            String::from("/v2/groups/g1/messages"),
            String::from("/v2/groups/g1/messages"),
            String::from("/v2/users/u1/messages"),
        ],
        "被动 → 主动 → 私聊三层"
    );
    assert_eq!(outcome.target.scene, Scene::C2c);
    assert!(outcome.delivered_active);
}

#[tokio::test]
async fn disabled_fallback_returns_original_error() {
    let server = MockServer::with_script(vec![
        MockResponse::platform(40034031, "消息已过期，不能回复"),
        MockResponse::ok_id("m2"),
    ])
    .await;
    let api = server.api();
    let outbound = Outbound::text("x")
        .with_msg_id("m1")
        .disable_active_fallback();
    let err = api.send(&group_target(), &outbound).await.unwrap_err();
    assert_eq!(err.code(), Some(40034031));
    assert_eq!(server.api_count(), 1, "关掉降级就不该再发");
}
