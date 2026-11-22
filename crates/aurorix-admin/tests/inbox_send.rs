use std::sync::Arc;

use aurorix_admin::{
    AdminState, ApiError, Conversation, InboxSendError, InboxSource, SourceFuture,
};
use aurorix_core::Target;
use tower::ServiceExt;

struct FakeInbox {
    outcome: InboxSendError,
}

impl InboxSource for FakeInbox {
    fn lookup(&self, conv_id: &str) -> Option<Conversation> {
        (conv_id == "g1").then(|| Conversation {
            id: String::from("g1"),
            kind: String::from("group"),
            name: String::from("测试群"),
            last_at: String::from("2024-01-01T00:00:00Z"),
            last_preview: String::from("hi"),
        })
    }

    fn send_async<'a>(
        &'a self,
        conv: &'a Conversation,
        text: &'a str,
    ) -> SourceFuture<'a, Result<(), InboxSendError>> {
        let failure = match &self.outcome {
            InboxSendError::NotConnected => Some(InboxSendError::NotConnected),
            InboxSendError::Failed(reason) => Some(InboxSendError::Failed(reason.clone())),
        };
        let recorded = format!("{}|{}", conv.id, text);
        Box::pin(async move {
            if recorded.is_empty() {
                return Err(InboxSendError::Failed(String::from("未收到内容")));
            }
            match failure {
                Some(error) => Err(error),
                None => Ok(()),
            }
        })
    }
}

const PASSWORD: &str = "inbox-test-pwd";

fn router(name: &str, inbox: Arc<dyn InboxSource>) -> axum::Router {
    let dir =
        std::env::temp_dir().join(format!("aurorix-admin-inbox-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("config.json");
    std::fs::write(
        &path,
        format!(r#"{{"port":8080,"admin_password":"{PASSWORD}"}}"#),
    )
    .expect("写配置文件");
    let config = aurorix_db::AppConfig::load(&path).expect("读配置文件");
    aurorix_admin::router(AdminState::new(&path, config).with_inbox(inbox))
}

async fn login(app: &axum::Router) -> String {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/admin/api/login")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({ "password": PASSWORD }).to_string(),
        ))
        .expect("请求应当可以构造");
    let response = app.clone().oneshot(request).await.expect("路由应当响应");
    assert_eq!(response.status().as_u16(), 200, "登录必须成功");
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("响应体应当可以读取");
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("登录响应应当是 JSON");
    body["token"].as_str().expect("需返回 token").to_string()
}

async fn post(app: &axum::Router, body: serde_json::Value) -> (u16, serde_json::Value) {
    let token = login(app).await;
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/admin/api/inbox/messages")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(body.to_string()))
        .expect("请求应当可以构造");
    let response = app.clone().oneshot(request).await.expect("路由应当响应");
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("响应体应当可以读取");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn missing_conversation_is_not_found() {
    let inbox = Arc::new(FakeInbox {
        outcome: InboxSendError::Failed(String::from("不应走到发送")),
    });
    let (status, body) = post(
        &router("missing", inbox),
        serde_json::json!({ "conv": "nope", "text": "hi" }),
    )
    .await;
    assert_eq!(status, 404, "会话不存在时应当返回 404");
    assert_eq!(body["error"], "会话不存在");
}

#[tokio::test]
async fn empty_payload_is_rejected() {
    let inbox = Arc::new(FakeInbox {
        outcome: InboxSendError::Failed(String::from("不应走到发送")),
    });
    let (status, body) = post(
        &router("empty", inbox),
        serde_json::json!({ "conv": "g1", "text": " " }),
    )
    .await;
    assert_eq!(status, 400, "内容为空时应当返回 400");
    assert_eq!(body["error"], "会话和内容都不能为空");
}

#[tokio::test]
async fn missing_transport_reports_not_connected() {
    let inbox = Arc::new(FakeInbox {
        outcome: InboxSendError::NotConnected,
    });
    let (status, body) = post(
        &router("no-transport", inbox),
        serde_json::json!({ "conv": "g1", "text": "你好" }),
    )
    .await;
    assert_eq!(status, 503, "没有任何发送通道时应当明确报告机器人未连接");
    assert_eq!(body["error"], "机器人未连接");
}

#[tokio::test]
async fn transport_failure_becomes_bad_gateway() {
    let inbox = Arc::new(FakeInbox {
        outcome: InboxSendError::Failed(String::from("平台返回 40034025")),
    });
    let (status, body) = post(
        &router("bad-gateway", inbox),
        serde_json::json!({ "conv": "g1", "text": "你好" }),
    )
    .await;
    assert_eq!(status, 502, "上游发送失败时应当返回 502");
    assert_eq!(
        body["error"], "平台返回 40034025",
        "上游错误信息应当原样透出"
    );
}

#[tokio::test]
async fn send_reaches_the_transport_with_target() {
    struct RecordingInbox {
        seen: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl InboxSource for RecordingInbox {
        fn lookup(&self, conv_id: &str) -> Option<Conversation> {
            (conv_id == "g1").then(|| Conversation {
                id: String::from("g1"),
                kind: String::from("group"),
                name: String::from("测试群"),
                last_at: String::new(),
                last_preview: String::new(),
            })
        }

        fn send_async<'a>(
            &'a self,
            conv: &'a Conversation,
            text: &'a str,
        ) -> SourceFuture<'a, Result<(), InboxSendError>> {
            let seen = Arc::clone(&self.seen);
            let record = format!("{}:{}:{}", conv.kind, conv.id, text);
            Box::pin(async move {
                seen.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(record);
                Ok(())
            })
        }
    }

    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (status, body) = post(
        &router(
            "recording",
            Arc::new(RecordingInbox {
                seen: Arc::clone(&seen),
            }),
        ),
        serde_json::json!({ "conv": "g1", "text": "管理台喊话" }),
    )
    .await;
    assert_eq!(status, 200, "成功发送应当返回 200");
    assert_eq!(body["ok"], true);
    assert_eq!(
        seen.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_slice(),
        ["group:g1:管理台喊话"],
        "会话类型、会话 id 与正文都必须原样送到传输层"
    );
}

#[test]
fn not_connected_maps_to_service_unavailable() {
    let error = ApiError::Unavailable(String::from("机器人未连接"));
    assert_eq!(
        error.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "未连接必须映射为 503"
    );
}

#[test]
fn target_keeps_scene_and_id() {
    let target = Target::new(aurorix_core::Scene::Group, "g1");
    assert_eq!(target.scene, aurorix_core::Scene::Group);
    assert_eq!(target.id, "g1");
    assert!(target.user.is_empty(), "主动消息不带用户标识");
}
