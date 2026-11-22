use reqwest::Method;
use serde_json::json;

use crate::api::BotApi;
use crate::error::ApiError;

pub const INTERACTION_ACK_CODE: i64 = 0;

pub const INTERACTION_ACK_PATH: &str = "/interactions/";

pub fn interaction_path(interaction_id: &str) -> String {
    format!("{INTERACTION_ACK_PATH}{interaction_id}")
}

impl BotApi {
    pub async fn ack_interaction(&self, interaction_id: &str) -> Result<(), ApiError> {
        if interaction_id.is_empty() {
            return Err(ApiError::InvalidTarget("交互 ID 为空"));
        }
        let url = self.endpoint(&interaction_path(interaction_id));
        let body = serde_json::to_vec(&json!({ "code": INTERACTION_ACK_CODE }))?;
        self.do_bytes(Method::PUT, &url, Some(body.as_slice()))
            .await
            .map(|_| ())
    }
}
