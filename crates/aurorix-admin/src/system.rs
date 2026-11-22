use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::error::ApiError;
use crate::state::AdminState;

pub const UNSUPERVISED_RESTART_ERROR: &str = "未声明受外部守护(AURORIX_SUPERVISED=1), 面板重启会与守护进程各拉一个实例互杀; 请手动重启或在受管模式下使用";

pub const CONTROL_DELAY_MS: u64 = 300;

pub type ControlAction = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Default)]
pub struct Control {
    pub supervised: bool,
    pub restart: Option<ControlAction>,
    pub stop: Option<ControlAction>,
}

impl Control {
    pub fn supervised(restart: Option<ControlAction>, stop: Option<ControlAction>) -> Self {
        Self {
            supervised: true,
            restart,
            stop,
        }
    }
}

fn trigger(action: Option<ControlAction>, name: &str) -> Result<Response, ApiError> {
    let Some(action) = action else {
        return Err(ApiError::NotImplemented(format!("{name}控制未启用")));
    };
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(CONTROL_DELAY_MS)).await;
        action();
    });
    let body = json!({ "ok": true, "notice": format!("{name}已触发") });
    Ok((StatusCode::ACCEPTED, Json(body)).into_response())
}

pub async fn restart(State(state): State<AdminState>) -> Result<Response, ApiError> {
    if !state.control.supervised {
        return Err(ApiError::Conflict(UNSUPERVISED_RESTART_ERROR.to_string()));
    }
    trigger(state.control.restart.clone(), "重启")
}

pub async fn stop(State(state): State<AdminState>) -> Result<Response, ApiError> {
    trigger(state.control.stop.clone(), "停止")
}
