use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::{invalid_body, ApiError};
use crate::sources::{SourceError, SourceFuture};
use crate::state::AdminState;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssetField {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssetProvider {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub has_secrets: bool,
    #[serde(default)]
    pub schema: Vec<AssetField>,
    #[serde(default)]
    pub config: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssetsView {
    #[serde(default)]
    pub whitelist: Vec<String>,
    #[serde(default)]
    pub providers: Vec<AssetProvider>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AssetProviderInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub config: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AssetsInput {
    #[serde(default)]
    pub whitelist: Vec<String>,
    #[serde(default)]
    pub providers: Vec<AssetProviderInput>,
}

pub trait AssetSource: Send + Sync {
    fn view(&self) -> AssetsView {
        AssetsView::default()
    }

    fn save(&self, _input: AssetsInput) -> Result<(), String> {
        Ok(())
    }

    fn view_async(&self) -> SourceFuture<'_, Result<AssetsView, SourceError>> {
        Box::pin(async move { Ok(self.view()) })
    }

    fn save_async(&self, input: AssetsInput) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move { self.save(input).map_err(SourceError::from_message) })
    }

    fn reload_async(&self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move { Ok(()) })
    }
}

pub trait AssetReload: Send + Sync {
    fn reload(&self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move { Ok(()) })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyAssets;

impl AssetSource for EmptyAssets {}

pub async fn get(State(state): State<AdminState>) -> Result<Json<AssetsView>, ApiError> {
    Ok(Json(state.assets.view_async().await?))
}

pub async fn put(
    State(state): State<AdminState>,
    payload: Result<Json<AssetsInput>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| invalid_body())?;
    state.assets.save_async(input).await?;
    state.assets.reload_async().await?;
    Ok(Json(json!({ "ok": true })))
}
