use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{invalid_body, ApiError};
use crate::state::AdminState;

fn is_zero(value: &i64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub plugin_id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cron: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub interval_ms: i64,
    #[serde(default)]
    pub immediate: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub last_fire: i64,
    #[serde(default)]
    pub next_fire: i64,
}

pub trait JobSource: Send + Sync {
    fn jobs(&self) -> Vec<JobInfo> {
        Vec::new()
    }

    fn exists(&self, id: &str) -> bool {
        self.jobs().iter().any(|job| job.id == id)
    }

    fn set_paused(&self, _id: &str, _paused: bool) {}
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyJobs;

impl JobSource for EmptyJobs {}

pub async fn list(State(state): State<AdminState>) -> Json<Vec<JobInfo>> {
    Json(state.jobs.jobs())
}

#[derive(Debug, Default, Deserialize)]
pub struct PauseRequest {
    #[serde(default)]
    pub paused: bool,
}

pub async fn pause(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    payload: Result<Json<PauseRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| invalid_body())?;
    if !state.jobs.exists(&id) {
        return Err(ApiError::NotFound("任务不存在".to_string()));
    }
    state.jobs.set_paused(&id, input.paused);
    Ok(Json(json!({ "ok": true })))
}
