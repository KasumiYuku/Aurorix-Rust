use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::state::AdminState;

pub trait ProfileSource: Send + Sync {
    fn available(&self) -> bool {
        false
    }

    fn get(&self) -> Option<Value> {
        None
    }

    fn refresh(&self) -> Result<Value, String> {
        Err("档案服务未启用".to_string())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyProfile;

impl ProfileSource for EmptyProfile {}

pub async fn get(State(state): State<AdminState>) -> Json<Value> {
    Json(json!({ "profile": state.profile.get() }))
}

pub async fn refresh(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    if !state.profile.available() {
        return Err(ApiError::Unavailable("档案服务未启用".to_string()));
    }
    let profile = state.profile.refresh().map_err(ApiError::BadGateway)?;
    Ok(Json(json!({ "profile": profile })))
}
