use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use aurorix_core::{Scene, Target};
use aurorix_protocol::{MessageKind, SendRequest, DEFAULT_RETRY_WHEN};

use crate::audit::AuditRegistry;
use crate::error::ApiError;
use crate::image_box::{clamp_markdown, ImageBox};
use crate::message::{DeliveryStep, Outbound, ResolvedAnchor, SendOutcome, MAX_PASSIVE_SEQ};
use crate::ratelimit::{RateLimiter, PUSH_KEY};
use crate::recall::RECALL_GRACE;
use crate::text::{guild_text, split_images};
use crate::token::{TokenManager, DEFAULT_TOKEN_URL};
use crate::upload::DEFAULT_UPLOAD_THRESHOLD;

pub const DEFAULT_BASE_URL: &str = "https://api.bot.qq.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotConfig {
    pub app_id: String,
    pub app_secret: String,
    pub base_url: String,
    pub token_url: String,
}

impl BotConfig {
    pub fn new(app_id: impl Into<String>, app_secret: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            app_secret: app_secret.into(),
            base_url: String::from(DEFAULT_BASE_URL),
            token_url: String::from(DEFAULT_TOKEN_URL),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_token_url(mut self, token_url: impl Into<String>) -> Self {
        self.token_url = token_url.into();
        self
    }
}

pub struct BotApi {
    config: BotConfig,
    http: reqwest::Client,
    tokens: TokenManager,
    retry_when: ArcSwap<Vec<i32>>,
    upload_threshold: AtomicUsize,
    audit: AuditRegistry,
    limiter: RateLimiter,
    recall_grace_ms: AtomicU64,
    image_box: ArcSwap<ImageBox>,
}

impl BotApi {
    pub fn new(config: BotConfig) -> Self {
        let http = reqwest::Client::new();
        let tokens = TokenManager::new(
            config.app_id.clone(),
            config.app_secret.clone(),
            config.token_url.clone(),
            http.clone(),
        );
        Self {
            config,
            http,
            tokens,
            retry_when: ArcSwap::from_pointee(DEFAULT_RETRY_WHEN.to_vec()),
            upload_threshold: AtomicUsize::new(DEFAULT_UPLOAD_THRESHOLD),
            audit: AuditRegistry::new(),
            limiter: RateLimiter::new(),
            recall_grace_ms: AtomicU64::new(RECALL_GRACE.as_millis() as u64),
            image_box: ArcSwap::from_pointee(ImageBox::default()),
        }
    }

    pub fn config(&self) -> &BotConfig {
        &self.config
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn tokens(&self) -> &TokenManager {
        &self.tokens
    }

    pub async fn access_token(&self) -> Result<String, ApiError> {
        self.tokens.get().await
    }

    pub fn invalidate_token(&self) {
        self.tokens.invalidate();
    }

    pub fn endpoint(&self, path: &str) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        format!("{base}{path}")
    }

    pub fn retry_when(&self) -> Arc<Vec<i32>> {
        self.retry_when.load_full()
    }

    pub fn set_retry_when(&self, codes: &[i32]) {
        self.retry_when.store(Arc::new(codes.to_vec()));
    }

    pub fn upload_threshold(&self) -> usize {
        self.upload_threshold.load(Ordering::SeqCst)
    }

    pub fn set_upload_threshold(&self, threshold: usize) {
        self.upload_threshold.store(threshold, Ordering::SeqCst);
    }

    pub fn image_box(&self) -> Arc<ImageBox> {
        self.image_box.load_full()
    }

    pub fn set_image_box(&self, image_box: ImageBox) {
        self.image_box.store(Arc::new(image_box));
    }

    pub fn audit(&self) -> &AuditRegistry {
        &self.audit
    }

    pub fn set_audit_timeout(&self, timeout: Duration) {
        self.audit.set_timeout(timeout);
    }

    pub fn resolve_audit(&self, audit_id: &str, message_id: &str, approved: bool) -> bool {
        self.audit.resolve(audit_id, message_id, approved)
    }

    pub(crate) async fn await_audit(&self, audit_id: &str) -> Result<(), ApiError> {
        self.audit.wait(audit_id).await.map(|_| ())
    }

    pub fn limiter(&self) -> &RateLimiter {
        &self.limiter
    }

    pub fn check_push_rate(&self) -> Result<(), ApiError> {
        self.limiter.check(PUSH_KEY)
    }

    pub fn recall_grace(&self) -> Duration {
        Duration::from_millis(self.recall_grace_ms.load(Ordering::SeqCst))
    }

    pub fn set_recall_grace(&self, grace: Duration) {
        self.recall_grace_ms
            .store(grace.as_millis() as u64, Ordering::SeqCst);
    }

    pub async fn send(&self, target: &Target, message: &Outbound) -> Result<SendOutcome, ApiError> {
        if target.empty() {
            return Err(ApiError::InvalidTarget("会话目标为空"));
        }
        let steps = delivery_steps(target, message);
        let first = &steps[0];
        match self.send_once(message, first).await {
            Ok(outcome) => Ok(outcome),
            Err(err) => {
                if !passive_gone(&err) || !message.active_fallback {
                    return Err(err);
                }
                let first = err.to_string();
                let mut last = err;
                for step in steps.iter().skip(1) {
                    match self.send_once(message, step).await {
                        Ok(mut outcome) => {
                            outcome.fallback = true;
                            return Ok(outcome);
                        }
                        Err(step_err) => last = step_err,
                    }
                }
                Err(ApiError::Fallback {
                    first,
                    last: Box::new(last),
                })
            }
        }
    }

    async fn send_once(
        &self,
        message: &Outbound,
        step: &DeliveryStep,
    ) -> Result<SendOutcome, ApiError> {
        let guild = step.target.scene.guild();
        if guild && message.kind == MessageKind::Media {
            return Err(ApiError::Unsupported {
                scene: step.target.scene,
                what: "语音/视频/文件消息",
            });
        }
        let anchor = resolve_anchor(message, step)?;
        let image_box = self.image_box.load();
        let request = build_request(message, step, &anchor, &image_box)?;
        let path = SendRequest::path_for_scene(step.target.scene, &step.target.id);
        let body = serde_json::to_vec(&request)?;
        let result = self.send_with_retry(&self.endpoint(&path), &body).await?;
        if guild {
            let (images, _) = split_images(&message.content);
            for url in images {
                self.send_channel_image(&step.target, &url, &anchor, step.active)
                    .await?;
            }
        }
        Ok(SendOutcome {
            message_id: result.message_id(),
            audit_id: result.audit(),
            target: step.target.clone(),
            delivered_active: step.active,
            fallback: false,
        })
    }

    pub async fn send_channel_image_bytes(
        &self,
        target: &Target,
        content: &str,
        msg_id: Option<&str>,
        filename: &str,
        mime_type: &str,
        data: &[u8],
    ) -> Result<Option<String>, ApiError> {
        if !target.scene.guild() {
            return Err(ApiError::Unsupported {
                scene: target.scene,
                what: "频道图片",
            });
        }
        if data.is_empty() {
            return Err(ApiError::Config(String::from("图片字节为空")));
        }
        let mut fields: Vec<(String, String)> =
            vec![(String::from("content"), content.to_string())];
        if let Some(msg_id) = msg_id {
            fields.push((String::from("msg_id"), msg_id.to_string()));
        }
        let path = SendRequest::path_for_scene(target.scene, &target.id);
        let url = self.endpoint(&path);
        let raw = self
            .post_multipart(&url, &fields, ("file_image", filename, mime_type, data))
            .await?;
        let result: crate::message::SendResult = serde_json::from_slice(&raw)?;
        Ok(result.message_id())
    }

    pub async fn send_channel_image_url(
        &self,
        target: &Target,
        content: &str,
        msg_id: Option<&str>,
        url: &str,
    ) -> Result<Option<String>, ApiError> {
        if !target.scene.guild() {
            return Err(ApiError::Unsupported {
                scene: target.scene,
                what: "频道图片",
            });
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ApiError::Config(String::from("频道图片 URL 非法")));
        }
        let mut body = serde_json::Map::new();
        body.insert(String::from("content"), serde_json::json!(content));
        if let Some(msg_id) = msg_id {
            body.insert(String::from("msg_id"), serde_json::json!(msg_id));
        }
        body.insert(String::from("image"), serde_json::json!(url));
        let path = SendRequest::path_for_scene(target.scene, &target.id);
        let raw = serde_json::to_vec(&serde_json::Value::Object(body))?;
        let result = self.send_with_retry(&self.endpoint(&path), &raw).await?;
        Ok(result.message_id())
    }

    async fn send_channel_image(
        &self,
        target: &Target,
        url: &str,
        anchor: &ResolvedAnchor,
        active: bool,
    ) -> Result<Option<String>, ApiError> {
        let mut body = serde_json::Map::new();
        body.insert(String::from("image"), serde_json::json!(url));
        if !active {
            if let Some(msg_id) = &anchor.msg_id {
                body.insert(String::from("msg_id"), serde_json::json!(msg_id));
            }
        }
        let path = SendRequest::path_for_scene(target.scene, &target.id);
        let raw = serde_json::to_vec(&serde_json::Value::Object(body))?;
        let result = self.send_with_retry(&self.endpoint(&path), &raw).await?;
        Ok(result.message_id())
    }

    pub async fn group_info(&self, group_id: &str) -> Result<GroupInfo, ApiError> {
        if group_id.is_empty() {
            return Err(ApiError::InvalidTarget("群 id 为空"));
        }
        let path = format!("/v2/groups/{group_id}/info");
        let raw = self
            .do_bytes(reqwest::Method::GET, &self.endpoint(&path), None)
            .await?;
        if raw.is_empty() {
            return Err(ApiError::Config(String::from("群资料返回为空")));
        }
        Ok(serde_json::from_slice(&raw)?)
    }
    pub async fn gateway_bot(&self) -> Result<GatewayBotInfo, ApiError> {
        let raw = self
            .do_bytes(reqwest::Method::GET, &self.endpoint("/gateway/bot"), None)
            .await?;
        if raw.is_empty() {
            return Err(ApiError::Config(String::from("网关信息返回为空")));
        }
        let info: GatewayBotInfo = serde_json::from_slice(&raw)?;
        if info.url.trim().is_empty() {
            return Err(ApiError::Config(String::from("网关地址为空")));
        }
        Ok(info)
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct GroupInfo {
    #[serde(default)]
    pub group_openid: String,
    #[serde(default)]
    pub group_name: String,
    #[serde(default)]
    pub group_finger_memo: String,
    #[serde(default)]
    pub group_class_text: String,
    #[serde(default)]
    pub group_tags: Vec<String>,
    #[serde(default)]
    pub group_member_num: i64,
}

impl GroupInfo {
    pub fn name(&self) -> &str {
        self.group_name.trim()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct SessionStartLimit {
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub remaining: i64,
    #[serde(default)]
    pub reset_after: i64,
    #[serde(default)]
    pub max_concurrency: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct GatewayBotInfo {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub shards: i64,
    #[serde(default)]
    pub session_start_limit: SessionStartLimit,
}

impl GatewayBotInfo {
    pub fn shard_count(&self) -> u32 {
        match u32::try_from(self.shards) {
            Ok(count) => count.max(1),
            Err(_) => 1,
        }
    }

    pub fn shard(&self) -> [u32; 2] {
        [0, self.shard_count()]
    }

    pub fn exhausted(&self) -> bool {
        self.session_start_limit.remaining <= 0 && self.session_start_limit.reset_after > 0
    }

    pub fn reset_after(&self) -> Duration {
        let millis = u64::try_from(self.session_start_limit.reset_after).unwrap_or(0);
        Duration::from_millis(millis)
    }
}

pub fn delivery_steps(target: &Target, message: &Outbound) -> Vec<DeliveryStep> {
    let steps = vec![
        DeliveryStep {
            target: target.clone(),
            active: message.initiative,
        },
        DeliveryStep {
            target: target.clone(),
            active: true,
        },
    ];
    let mut out = steps;
    if target.scene == Scene::Group && !target.user.is_empty() {
        out.push(DeliveryStep {
            target: Target::new(Scene::C2c, target.user.clone()).with_user(target.user.clone()),
            active: true,
        });
    }
    out
}

pub fn passive_gone(err: &ApiError) -> bool {
    matches!(err, ApiError::ReplyBudgetExhausted { .. }) || err.is_passive_expired()
}

fn resolve_anchor(message: &Outbound, step: &DeliveryStep) -> Result<ResolvedAnchor, ApiError> {
    if step.active {
        return Ok(ResolvedAnchor::default());
    }
    if let Some(budget) = &message.budget {
        if budget.spent() {
            return Err(ApiError::ReplyBudgetExhausted {
                limit: MAX_PASSIVE_SEQ,
            });
        }
    }
    let seq = match &message.budget {
        Some(budget) => Some(budget.count()?),
        None => None,
    };
    let mut anchor = ResolvedAnchor {
        msg_id: optional(&message.anchor.msg_id),
        event_id: optional(&message.anchor.event_id),
        seq,
    };
    if step.target.scene.guild() {
        anchor.event_id = None;
        anchor.seq = None;
    }
    Ok(anchor)
}

fn optional(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn build_request(
    message: &Outbound,
    step: &DeliveryStep,
    anchor: &ResolvedAnchor,
    image_box: &ImageBox,
) -> Result<SendRequest, ApiError> {
    let guild = step.target.scene.guild();
    let clampable =
        !guild && matches!(message.kind, MessageKind::Markdown | MessageKind::PlainText);
    let clamped = clampable.then(|| clamp_markdown(&message.content, image_box));
    let content = clamped.as_deref().unwrap_or(&message.content);
    let mut request = if guild {
        SendRequest::new(guild_text(content), MessageKind::PlainText)
    } else {
        match message.kind {
            MessageKind::Markdown => {
                let mut request = SendRequest::new(String::new(), MessageKind::Markdown);
                request.markdown = Some(serde_json::json!({ "content": content }));
                request
            }
            MessageKind::Media => {
                let media = message
                    .media
                    .clone()
                    .ok_or_else(|| ApiError::Config(String::from("媒体消息缺少 media 字段")))?;
                let mut request = SendRequest::new(content, MessageKind::Media);
                request.media = Some(media);
                request
            }
            MessageKind::PlainText => SendRequest::new(content, MessageKind::PlainText),
        }
    };
    if !guild {
        if let Some(keyboard) = &message.keyboard {
            request = request.with_keyboard(keyboard.clone());
        }
    }
    if !step.active {
        if let Some(message_id) = &message.reference {
            request = request.with_reference(message_id.clone());
        }
    }
    if let Some(msg_id) = &anchor.msg_id {
        request = request.with_msg_id(msg_id.clone());
    }
    if let Some(event_id) = &anchor.event_id {
        request = request.with_event_id(event_id.clone());
    }
    if let Some(seq) = anchor.seq {
        request = request.with_msg_seq(seq);
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Anchor;

    #[test]
    fn default_config_points_at_official_hosts() {
        let config = BotConfig::new("app", "secret");
        assert_eq!(config.base_url, "https://api.bot.qq.com");
        assert_eq!(
            config.token_url,
            "https://bots.qq.com/app/getAppAccessToken"
        );
        let api = BotApi::new(config);
        assert_eq!(
            api.endpoint("/v2/groups/g1/messages"),
            "https://api.bot.qq.com/v2/groups/g1/messages"
        );
        assert_eq!(api.upload_threshold(), DEFAULT_UPLOAD_THRESHOLD);
        assert_eq!(api.retry_when().as_slice(), DEFAULT_RETRY_WHEN.as_slice());
        assert_eq!(api.recall_grace(), RECALL_GRACE);
        api.set_retry_when(&[42]);
        assert_eq!(api.retry_when().as_slice(), &[42]);
    }

    #[test]
    fn base_url_trailing_slash_is_ignored() {
        let api = BotApi::new(BotConfig::new("app", "secret").with_base_url("http://127.0.0.1:1/"));
        assert_eq!(api.endpoint("/x"), "http://127.0.0.1:1/x");
    }

    #[test]
    fn steps_are_passive_then_active_then_private() {
        let group = Target::new(Scene::Group, "g1");
        let steps = delivery_steps(&group, &Outbound::text("x"));
        assert_eq!(steps.len(), 2, "群聊没有私聊落点时不加第三层");
        assert!(!steps[0].active);
        assert!(steps[1].active);

        let with_user = Target::new(Scene::Group, "g1").with_user("u1");
        let steps = delivery_steps(&with_user, &Outbound::text("x"));
        assert_eq!(steps.len(), 3, "群聊要能退到单聊");
        assert_eq!(steps[2].target.scene, Scene::C2c);
        assert_eq!(steps[2].target.id, "u1");
        assert!(steps[2].active);

        let initiative = delivery_steps(&group, &Outbound::text("x").as_initiative());
        assert!(initiative[0].active, "主动消息第一层就是主动");
    }

    #[test]
    fn passive_gone_only_matches_expiry_and_budget() {
        let expired = ApiError::Platform(crate::error::BusinessError::new(40034031, "过期"));
        assert!(passive_gone(&expired));
        let budget = ApiError::ReplyBudgetExhausted { limit: 5 };
        assert!(passive_gone(&budget), "预算用尽也算被动消失");
        let violation = ApiError::Platform(crate::error::BusinessError::new(40034006, "违规"));
        assert!(!passive_gone(&violation), "内容违规不能降级");
        let param = ApiError::Platform(crate::error::BusinessError::new(40034028, "链接"));
        assert!(!passive_gone(&param));
    }

    #[test]
    fn passive_anchor_keeps_seq_active_clears_it() {
        let budget = crate::message::SeqBudget::shared();
        let outbound = Outbound::text("x")
            .with_anchor(Anchor::new("m1", "e1"))
            .with_budget(Arc::clone(&budget));
        let target = Target::new(Scene::Group, "g1");
        let steps = delivery_steps(&target, &outbound);
        let passive = resolve_anchor(&outbound, &steps[0]).unwrap();
        assert_eq!(passive.msg_id.as_deref(), Some("m1"));
        assert_eq!(passive.event_id.as_deref(), Some("e1"));
        assert_eq!(passive.seq, Some(1));
        let active = resolve_anchor(&outbound, &steps[1]).unwrap();
        assert_eq!(active, ResolvedAnchor::default(), "主动通道要清空锚点");
    }

    #[test]
    fn guild_anchor_drops_event_id_and_seq() {
        let budget = crate::message::SeqBudget::shared();
        let outbound = Outbound::markdown("x")
            .with_anchor(Anchor::new("m1", "e1"))
            .with_budget(Arc::clone(&budget));
        let target = Target::new(Scene::Channel, "c1");
        let steps = delivery_steps(&target, &outbound);
        let anchor = resolve_anchor(&outbound, &steps[0]).unwrap();
        assert_eq!(anchor.msg_id.as_deref(), Some("m1"), "频道必须保留 msg_id");
        assert_eq!(anchor.event_id, None, "频道不带 event_id");
        assert_eq!(anchor.seq, None, "频道不带 msg_seq");
    }

    #[test]
    fn guild_request_downgrades_markdown() {
        let outbound = Outbound::markdown("# 标题\nhttps://evil/x\n正文").with_msg_id("m1");
        let steps = delivery_steps(&Target::new(Scene::Channel, "c1"), &outbound);
        let anchor = resolve_anchor(&outbound, &steps[0]).unwrap();
        let request =
            build_request(&outbound, &steps[0], &anchor, &ImageBox::default()).unwrap();
        assert_eq!(request.msg_type, 0, "频道没有 markdown 通道");
        assert_eq!(request.content, "标题\n正文");
        assert!(request.markdown.is_none());
        assert_eq!(request.msg_id.as_deref(), Some("m1"));
    }

    #[test]
    fn group_markdown_request_keeps_markdown_and_keyboard() {
        let keyboard = aurorix_buttons::Builder::new()
            .command("k", "按钮", "/k", Vec::new())
            .build()
            .unwrap();
        let outbound = Outbound::markdown("**粗**")
            .with_keyboard(&keyboard)
            .unwrap()
            .with_msg_id("m1")
            .with_budget(crate::message::SeqBudget::shared());
        let steps = delivery_steps(&Target::new(Scene::Group, "g1"), &outbound);
        let anchor = resolve_anchor(&outbound, &steps[0]).unwrap();
        let request =
            build_request(&outbound, &steps[0], &anchor, &ImageBox::default()).unwrap();
        assert_eq!(request.msg_type, 2);
        assert_eq!(request.markdown.as_ref().unwrap()["content"], "**粗**");
        assert_eq!(request.content, "");
        assert!(request.keyboard.is_some());
        assert_eq!(request.msg_seq, Some(1));
    }

    #[test]
    fn markdown_images_are_clamped_before_the_request_is_built() {
        let outbound = Outbound::markdown("![卡片 #1000px #2000px](https://e/a.png)");
        let steps = delivery_steps(&Target::new(Scene::Group, "g1"), &outbound);
        let anchor = resolve_anchor(&outbound, &steps[0]).unwrap();
        let request =
            build_request(&outbound, &steps[0], &anchor, &ImageBox::default()).unwrap();
        assert_eq!(
            request.markdown.as_ref().unwrap()["content"],
            "![卡片 #350px #438px](https://e/a.png)"
        );
        let off = build_request(&outbound, &steps[0], &anchor, &ImageBox::off()).unwrap();
        assert_eq!(
            off.markdown.as_ref().unwrap()["content"],
            "![卡片 #1000px #2000px](https://e/a.png)",
            "关掉上限就原样发"
        );
        let media = Outbound::media("![卡片 #1000px #2000px](https://e/a.png)", serde_json::json!({"file_info": "f1"}));
        let media_steps = delivery_steps(&Target::new(Scene::Group, "g1"), &media);
        let media_request = build_request(
            &media,
            &media_steps[0],
            &ResolvedAnchor::default(),
            &ImageBox::default(),
        )
        .unwrap();
        assert_eq!(
            media_request.content, "![卡片 #1000px #2000px](https://e/a.png)",
            "媒体消息正文不参与夹取"
        );
    }

    #[test]
    fn media_request_requires_payload() {
        let outbound = Outbound::media("图", serde_json::json!({"file_info": "f1"}));
        let steps = delivery_steps(&Target::new(Scene::Group, "g1"), &outbound);
        let request = build_request(
            &outbound,
            &steps[0],
            &ResolvedAnchor::default(),
            &ImageBox::default(),
        )
        .unwrap();
        assert_eq!(request.msg_type, 7);
        assert_eq!(request.media.as_ref().unwrap()["file_info"], "f1");
    }

    #[test]
    fn media_passive_request_serializes_anchor_without_content() {
        let outbound = Outbound::media("", serde_json::json!({ "file_info": "f1" }))
            .with_msg_id("m1")
            .with_budget(crate::message::SeqBudget::shared());
        let target = Target::new(Scene::Group, "g1");
        let steps = delivery_steps(&target, &outbound);
        let anchor = resolve_anchor(&outbound, &steps[0]).unwrap();
        let request =
            build_request(&outbound, &steps[0], &anchor, &ImageBox::default()).unwrap();
        let raw = serde_json::to_value(&request).unwrap();
        assert_eq!(raw["msg_type"], 7);
        assert_eq!(raw["msg_id"], "m1", "被动回复必须带 msg_id");
        assert_eq!(raw["msg_seq"], 1, "被动回复必须带 msg_seq");
        assert!(
            raw.get("content").is_none(),
            "没有正文时不该发空 content, 实际 {raw}"
        );
        assert_eq!(raw["media"]["file_info"], "f1");
    }

    #[test]
    fn fallback_error_explains_both_attempts() {
        let first = ApiError::Platform(crate::error::BusinessError::new(
            40034005,
            "回复消息 msg_id 已过期",
        ));
        let last = ApiError::Platform(crate::error::BusinessError::new(
            40034105,
            "主动消息发送失败，无权限",
        ));
        let err = ApiError::Fallback {
            first: first.to_string(),
            last: Box::new(last),
        };
        assert_eq!(err.code(), Some(40034105), "分类要跟随最终错误");
        let text = err.to_string();
        assert!(
            text.contains("msg_id 已过期"),
            "要能看出被动回复那次失败的原因: {text}"
        );
        assert!(text.contains("无权限"), "要保留最终错误: {text}");
    }
}
