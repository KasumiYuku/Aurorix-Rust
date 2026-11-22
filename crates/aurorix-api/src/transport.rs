use std::time::Duration;

use reqwest::Method;
use serde::Deserialize;

use crate::api::BotApi;
use crate::audit::AUDIT_PENDING_CODE;
use crate::error::{is_retryable, ApiError, BusinessError};
use crate::message::SendResult;

pub const EVENT_ID_RETRY_CODE: i32 = 40034025;
pub const EVENT_ID_RETRY_DELAY: Duration = Duration::from_millis(200);
pub const EVENT_ID_RETRY_LIMIT: u32 = 3;
pub const RETRY_LIMIT: u32 = 2;
pub const RETRY_BASE_DELAY: Duration = Duration::from_millis(300);

const AUTH_HEADER: &str = "Authorization";
const TOKEN_PREFIX: &str = "QQBot ";

#[derive(Debug, Deserialize)]
struct Envelope {
    code: i32,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct AuditEnvelope {
    code: i32,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: AuditData,
}

#[derive(Debug, Default, Deserialize)]
struct AuditData {
    #[serde(default)]
    message_audit: Option<AuditInfo>,
}

#[derive(Debug, Deserialize)]
struct AuditInfo {
    #[serde(default)]
    audit_id: String,
}

pub fn parse_business_error(raw: &[u8]) -> Option<BusinessError> {
    let envelope: Envelope = serde_json::from_slice(raw).ok()?;
    if envelope.code == 0 {
        return None;
    }
    Some(BusinessError::new(envelope.code, envelope.message))
}

fn status_error(status: u16, raw: &[u8]) -> ApiError {
    match parse_business_error(raw) {
        Some(err) => ApiError::Platform(err),
        None => ApiError::Status {
            status,
            body: String::from_utf8_lossy(raw).to_string(),
        },
    }
}

impl BotApi {
    pub async fn do_bytes(
        &self,
        method: Method,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<Vec<u8>, ApiError> {
        let mut attempt = 0u32;
        loop {
            let token = self.access_token().await?;
            let mut request = self
                .http()
                .request(method.clone(), url)
                .header(AUTH_HEADER, format!("{TOKEN_PREFIX}{token}"))
                .header("Content-Type", "application/json");
            if let Some(raw) = body {
                request = request.body(raw.to_vec());
            }
            let response = request.send().await?;
            let status = response.status();
            let raw = response.bytes().await?.to_vec();
            if status.is_success() {
                return Ok(raw);
            }
            let code = status.as_u16();
            if attempt == 0 && code == 401 {
                self.invalidate_token();
                attempt += 1;
                continue;
            }
            return Err(status_error(code, &raw));
        }
    }

    pub async fn post_multipart(
        &self,
        url: &str,
        fields: &[(String, String)],
        file: (&str, &str, &str, &[u8]),
    ) -> Result<Vec<u8>, ApiError> {
        let mut attempt = 0u32;
        loop {
            let token = self.access_token().await?;
            let (field, filename, mime_type, data) = file;
            let mut form = reqwest::multipart::Form::new();
            for (name, value) in fields {
                form = form.text(name.clone(), value.clone());
            }
            let part = reqwest::multipart::Part::bytes(data.to_vec())
                .file_name(filename.to_string())
                .mime_str(mime_type)?;
            form = form.part(field.to_string(), part);
            let response = self
                .http()
                .post(url)
                .header(AUTH_HEADER, format!("{TOKEN_PREFIX}{token}"))
                .multipart(form)
                .send()
                .await?;
            let status = response.status();
            let raw = response.bytes().await?.to_vec();
            if status.is_success() {
                return Ok(raw);
            }
            let code = status.as_u16();
            if attempt == 0 && code == 401 {
                self.invalidate_token();
                attempt += 1;
                continue;
            }
            return Err(status_error(code, &raw));
        }
    }

    pub async fn do_json_within(
        &self,
        method: Method,
        url: &str,
        body: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, ApiError> {
        match tokio::time::timeout(timeout, self.do_bytes(method, url, Some(body))).await {
            Ok(result) => result,
            Err(_) => Err(ApiError::Upload(format!("请求超时 {timeout:?}"))),
        }
    }

    pub async fn put_presigned(
        &self,
        url: &str,
        chunk: &[u8],
        mime_type: &str,
        timeout: Duration,
    ) -> Result<(), ApiError> {
        let mut request = self.http().put(url).body(chunk.to_vec());
        if !mime_type.is_empty() {
            request = request.header("Content-Type", mime_type);
        }
        let outcome = tokio::time::timeout(timeout, request.send()).await;
        let response = match outcome {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => return Err(ApiError::Transport(err)),
            Err(_) => return Err(ApiError::Upload(format!("分片上传超时 {timeout:?}"))),
        };
        let status = response.status();
        let raw = response.bytes().await?.to_vec();
        if status.is_success() {
            return Ok(());
        }
        Err(status_error(status.as_u16(), &raw))
    }

    pub(crate) async fn send_with_retry(
        &self,
        endpoint: &str,
        body: &[u8],
    ) -> Result<SendResult, ApiError> {
        let mut attempt = 0u32;
        loop {
            let outcome = self.do_bytes(Method::POST, endpoint, Some(body)).await;
            let raw = match outcome {
                Ok(raw) => raw,
                Err(err) => {
                    let retry_when = self.retry_when();
                    if let Some(next) = retry_after_error(&err, attempt, retry_when.as_slice()) {
                        tokio::time::sleep(next).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            };
            if raw.is_empty() {
                return Ok(SendResult::default());
            }
            let parsed: SendResult = serde_json::from_slice(&raw).unwrap_or_default();
            if !parsed.id.is_empty() {
                return Ok(parsed);
            }
            let envelope: Option<AuditEnvelope> = serde_json::from_slice(&raw).ok();
            let Some(envelope) = envelope else {
                return Ok(parsed);
            };
            if envelope.code == 0 {
                return Ok(parsed);
            }
            if envelope.code == AUDIT_PENDING_CODE {
                if let Some(info) = envelope.data.message_audit {
                    self.await_audit(&info.audit_id).await?;
                    return Ok(parsed);
                }
            }
            let err = ApiError::Platform(BusinessError::new(envelope.code, envelope.message));
            let retry_when = self.retry_when();
            if err.is_retryable(retry_when.as_slice()) && attempt < RETRY_LIMIT {
                tokio::time::sleep(RETRY_BASE_DELAY * (attempt + 1)).await;
                attempt += 1;
                continue;
            }
            return Err(err);
        }
    }
}

fn retry_after_error(err: &ApiError, attempt: u32, retry_when: &[i32]) -> Option<Duration> {
    let code = err.code()?;
    if code == EVENT_ID_RETRY_CODE {
        if attempt < EVENT_ID_RETRY_LIMIT {
            return Some(EVENT_ID_RETRY_DELAY);
        }
        return None;
    }
    if !is_retryable(code, retry_when) {
        return None;
    }
    if attempt < RETRY_LIMIT {
        return Some(RETRY_BASE_DELAY * (attempt + 1));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn business_envelope_is_extracted() {
        let err = parse_business_error(br#"{"code":40034031,"message":"expired"}"#).unwrap();
        assert_eq!(err.code, 40034031);
        assert_eq!(err.message, "expired");
        assert!(err.is_passive_expired());
        assert!(parse_business_error(br#"{"code":0,"message":"ok"}"#).is_none());
        assert!(parse_business_error(b"not json").is_none());
    }

    #[test]
    fn status_error_prefers_business_code() {
        let platform = status_error(400, br#"{"code":40034025,"message":"event id invalid"}"#);
        assert_eq!(platform.code(), Some(40034025));
        let plain = status_error(502, b"bad gateway");
        match plain {
            ApiError::Status { status, body } => {
                assert_eq!(status, 502);
                assert_eq!(body, "bad gateway");
            }
            other => panic!("应回退到状态错误: {other}"),
        }
    }

    #[test]
    fn event_id_code_retries_three_times() {
        let err = ApiError::Platform(BusinessError::new(EVENT_ID_RETRY_CODE, "尚未落库"));
        assert_eq!(retry_after_error(&err, 0, &[]), Some(EVENT_ID_RETRY_DELAY));
        assert_eq!(retry_after_error(&err, 1, &[]), Some(EVENT_ID_RETRY_DELAY));
        assert_eq!(retry_after_error(&err, 2, &[]), Some(EVENT_ID_RETRY_DELAY));
        assert_eq!(retry_after_error(&err, 3, &[]), None, "第 3 次重试后放弃");
    }

    #[test]
    fn other_codes_use_short_backoff() {
        let err = ApiError::Platform(BusinessError::new(50055001, "稍后重试"));
        assert_eq!(retry_after_error(&err, 0, &[]), Some(RETRY_BASE_DELAY));
        assert_eq!(retry_after_error(&err, 1, &[]), Some(RETRY_BASE_DELAY * 2));
        assert_eq!(retry_after_error(&err, 2, &[]), None);
        assert_eq!(
            retry_after_error(&ApiError::InvalidTarget("x"), 0, &[]),
            None
        );
    }

    #[test]
    fn passive_expiry_is_never_retried() {
        let err = ApiError::Platform(BusinessError::new(40034031, "消息已过期"));
        assert_eq!(
            retry_after_error(&err, 0, &[40034031]),
            None,
            "被动过期只降级不重试"
        );
        let unlisted = ApiError::Platform(BusinessError::new(424242, "未知"));
        assert_eq!(retry_after_error(&unlisted, 0, &[]), None);
        assert_eq!(
            retry_after_error(&unlisted, 0, &[424242]),
            Some(RETRY_BASE_DELAY)
        );
    }
}
