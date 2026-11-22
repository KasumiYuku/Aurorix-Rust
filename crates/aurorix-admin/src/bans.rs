use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{invalid_body, ApiError};
use crate::sources::{SourceError, SourceFuture};
use crate::state::AdminState;

pub const DEFAULT_BAN_MESSAGE: &str = "您的账号因违反Bot使用协议已被封禁";

pub const BAN_MESSAGE_LIMIT: usize = 2000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BanEntry {
    #[serde(default)]
    pub openid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub at: String,
}

pub trait BanSource: Send + Sync {
    fn list(&self) -> Vec<BanEntry> {
        Vec::new()
    }

    fn put(&self, _entry: BanEntry) -> Result<(), String> {
        Ok(())
    }

    fn delete(&self, _openid: &str) -> Result<(), String> {
        Err("该用户不在封禁名单里".to_string())
    }

    fn list_async(&self) -> SourceFuture<'_, Result<Vec<BanEntry>, SourceError>> {
        Box::pin(async move { Ok(self.list()) })
    }

    fn put_async(&self, entry: BanEntry) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move { self.put(entry).map_err(SourceError::from_message) })
    }

    fn delete_async(&self, openid: String) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move { self.delete(&openid).map_err(SourceError::from_message) })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyBans;

impl BanSource for EmptyBans {}

fn validate(entry: &BanEntry) -> Result<(), String> {
    if entry.openid.trim().is_empty() {
        return Err("openid 不能为空".to_string());
    }
    if entry
        .openid
        .chars()
        .any(|ch| ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n')
    {
        return Err("openid 不能包含空白字符".to_string());
    }
    let length = entry.message.chars().count();
    if length > BAN_MESSAGE_LIMIT {
        return Err(format!("文案 {length} 字, 超过上限 {BAN_MESSAGE_LIMIT} 字"));
    }
    Ok(())
}

pub async fn list(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    let entries = state.bans.list_async().await?;
    Ok(Json(json!({
        "entries": entries,
        "default": DEFAULT_BAN_MESSAGE,
        "limit": BAN_MESSAGE_LIMIT,
    })))
}

#[derive(Debug, Default, Deserialize)]
pub struct BanRequest {
    #[serde(default)]
    pub openid: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub note: String,
}

pub async fn put(
    State(state): State<AdminState>,
    payload: Result<Json<BanRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| invalid_body())?;
    let entry = BanEntry {
        openid: input.openid.trim().to_string(),
        message: input.message,
        note: input.note,
        at: String::new(),
    };
    validate(&entry).map_err(ApiError::BadRequest)?;
    state.bans.put_async(entry).await?;
    Ok(Json(json!({
        "ok": true,
        "entries": state.bans.list_async().await?,
    })))
}

pub async fn delete(
    State(state): State<AdminState>,
    Path(openid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state.bans.delete_async(openid.trim().to_string()).await?;
    Ok(Json(json!({
        "ok": true,
        "entries": state.bans.list_async().await?,
    })))
}
