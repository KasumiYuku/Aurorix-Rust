use axum::extract::rejection::JsonRejection;
use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use aurorix_core::Target;

use crate::error::{invalid_body, ApiError};
use crate::sources::{SourceError, SourceFuture};
use crate::state::AdminState;

pub const DEFAULT_PAGE_SIZE: usize = 50;

pub const MAX_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub last_at: String,
    #[serde(default)]
    pub last_preview: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxMessage {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub conv_id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sender_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sender_name: String,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub platform_id: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InboxSendError {
    #[error("机器人未连接")]
    NotConnected,
    #[error("{0}")]
    Failed(String),
}

pub trait InboxSender: Send + Sync {
    fn send<'a>(
        &'a self,
        target: &'a Target,
        text: &'a str,
    ) -> SourceFuture<'a, Result<(), InboxSendError>>;
}

pub trait InboxSource: Send + Sync {
    fn conversations(&self) -> Result<Vec<Conversation>, String> {
        Ok(Vec::new())
    }

    fn messages(
        &self,
        _conv_id: &str,
        _before: Option<i64>,
        _limit: usize,
    ) -> Result<Vec<InboxMessage>, String> {
        Ok(Vec::new())
    }

    fn lookup(&self, _conv_id: &str) -> Option<Conversation> {
        None
    }

    fn send(&self, _conv: &Conversation, _text: &str) -> Result<(), InboxSendError> {
        Err(InboxSendError::NotConnected)
    }

    fn conversations_async(
        &self,
        limit: usize,
    ) -> SourceFuture<'_, Result<Vec<Conversation>, SourceError>> {
        Box::pin(async move {
            let mut list = self.conversations().map_err(SourceError::from_message)?;
            list.truncate(limit);
            Ok(list)
        })
    }

    fn messages_async(
        &self,
        conv_id: String,
        before: Option<i64>,
        limit: usize,
    ) -> SourceFuture<'_, Result<Vec<InboxMessage>, SourceError>> {
        Box::pin(async move {
            self.messages(&conv_id, before, limit)
                .map_err(SourceError::from_message)
        })
    }

    fn lookup_async(
        &self,
        conv_id: String,
    ) -> SourceFuture<'_, Result<Option<Conversation>, SourceError>> {
        Box::pin(async move { Ok(self.lookup(&conv_id)) })
    }

    fn send_async<'a>(
        &'a self,
        conv: &'a Conversation,
        text: &'a str,
    ) -> SourceFuture<'a, Result<(), InboxSendError>> {
        Box::pin(async move { self.send(conv, text) })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyInbox;

impl InboxSource for EmptyInbox {}

pub fn page_size(limit: Option<i64>) -> usize {
    match limit {
        Some(value) if value > 0 && (value as usize) <= MAX_PAGE_SIZE => value as usize,
        _ => DEFAULT_PAGE_SIZE,
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ConversationsQuery {
    #[serde(default)]
    limit: Option<i64>,
}

pub async fn conversations(
    State(state): State<AdminState>,
    Query(query): Query<ConversationsQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = page_size(query.limit);
    let list = state.inbox.conversations_async(limit).await?;
    Ok(Json(json!({ "conversations": list })))
}

#[derive(Debug, Default, Deserialize)]
pub struct MessageQuery {
    #[serde(default)]
    conv: String,
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

pub async fn messages(
    State(state): State<AdminState>,
    Query(query): Query<MessageQuery>,
) -> Result<Json<Value>, ApiError> {
    let conv = query.conv.trim().to_string();
    if conv.is_empty() {
        return Err(ApiError::BadRequest("缺少会话 id".to_string()));
    }
    let limit = page_size(query.limit);
    let list = state
        .inbox
        .messages_async(conv, query.before, limit)
        .await?;
    let has_more = list.len() == limit;
    Ok(Json(json!({ "messages": list, "has_more": has_more })))
}

#[derive(Debug, Default, Deserialize)]
pub struct SendRequest {
    #[serde(default)]
    pub conv: String,
    #[serde(default)]
    pub text: String,
}

pub async fn send(
    State(state): State<AdminState>,
    payload: Result<Json<SendRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| invalid_body())?;
    let conv_id = input.conv.trim().to_string();
    let text = input.text.trim().to_string();
    if conv_id.is_empty() || text.is_empty() {
        return Err(ApiError::BadRequest("会话和内容都不能为空".to_string()));
    }
    let Some(conv) = state.inbox.lookup_async(conv_id).await? else {
        return Err(ApiError::NotFound("会话不存在".to_string()));
    };
    match state.inbox.send_async(&conv, &text).await {
        Ok(()) => {
            state.live.publish(
                crate::live::TOPIC_INBOX,
                json!({
                    "conv": conv.id,
                    "scene": conv.kind,
                    "user": "",
                    "user_name": "",
                    "content": "",
                    "reply": text,
                    "platform_id": "",
                    "at": crate::live::now_ms(),
                }),
            );
            Ok(Json(json!({ "ok": true })))
        }
        Err(InboxSendError::NotConnected) => Err(ApiError::Unavailable("机器人未连接".to_string())),
        Err(InboxSendError::Failed(message)) => Err(ApiError::BadGateway(message)),
    }
}
