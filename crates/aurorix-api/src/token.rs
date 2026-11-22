use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::ApiError;

pub const DEFAULT_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
pub const EXPIRY_MARGIN: Duration = Duration::from_secs(50);
pub const FAILURE_COOLDOWN: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
struct Cached {
    token: String,
    expire_at: Option<Instant>,
}

impl Cached {
    fn valid(&self, now: Instant) -> bool {
        if self.token.is_empty() {
            return false;
        }
        match self.expire_at {
            Some(at) => now < at,
            None => false,
        }
    }
}

pub struct TokenManager {
    app_id: String,
    app_secret: String,
    endpoint: String,
    client: reqwest::Client,
    cached: ArcSwap<Cached>,
    gate: AsyncMutex<()>,
    failure: Mutex<Option<(Instant, String)>>,
}

impl TokenManager {
    pub fn new(
        app_id: impl Into<String>,
        app_secret: impl Into<String>,
        endpoint: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            app_secret: app_secret.into(),
            endpoint: endpoint.into(),
            client,
            cached: ArcSwap::from_pointee(Cached::default()),
            gate: AsyncMutex::new(()),
            failure: Mutex::new(None),
        }
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn invalidate(&self) {
        self.cached.store(Arc::new(Cached::default()));
        if let Ok(mut slot) = self.failure.lock() {
            *slot = None;
        }
    }

    pub async fn get(&self) -> Result<String, ApiError> {
        {
            let snapshot = self.cached.load();
            if snapshot.valid(Instant::now()) {
                return Ok(snapshot.token.clone());
            }
        }
        let _guard = self.gate.lock().await;
        {
            let snapshot = self.cached.load();
            if snapshot.valid(Instant::now()) {
                return Ok(snapshot.token.clone());
            }
        }
        if let Some((at, message)) = self.recent_failure() {
            if at.elapsed() < FAILURE_COOLDOWN {
                return Err(ApiError::Token(message));
            }
        }
        match self.fetch().await {
            Ok((token, ttl)) => {
                let expire_at = Instant::now().checked_add(ttl);
                self.cached.store(Arc::new(Cached {
                    token: token.clone(),
                    expire_at,
                }));
                if let Ok(mut slot) = self.failure.lock() {
                    *slot = None;
                }
                Ok(token)
            }
            Err(err) => {
                let message = err.to_string();
                if let Ok(mut slot) = self.failure.lock() {
                    *slot = Some((Instant::now(), message));
                }
                Err(err)
            }
        }
    }

    fn recent_failure(&self) -> Option<(Instant, String)> {
        self.failure.lock().ok().and_then(|slot| slot.clone())
    }

    async fn fetch(&self) -> Result<(String, Duration), ApiError> {
        let body = serde_json::json!({
            "appId": self.app_id,
            "clientSecret": self.app_secret,
        });
        let response = self.client.post(&self.endpoint).json(&body).send().await?;
        let status = response.status();
        let raw = response.text().await?;
        if !status.is_success() {
            return Err(ApiError::Status {
                status: status.as_u16(),
                body: raw,
            });
        }
        let payload: Value = serde_json::from_str(&raw)?;
        let token = payload
            .get("access_token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if token.is_empty() {
            return Err(ApiError::Token(String::from("access_token 响应为空")));
        }
        let expires_in = parse_expires_in(payload.get("expires_in"))?;
        let ttl = Duration::from_secs(expires_in).saturating_sub(EXPIRY_MARGIN);
        Ok((token, ttl))
    }
}

pub fn parse_expires_in(raw: Option<&Value>) -> Result<u64, ApiError> {
    let value = raw.ok_or_else(|| ApiError::Token(String::from("expires_in 缺失")))?;
    let seconds = match value {
        Value::String(text) => text
            .trim()
            .parse::<i64>()
            .map_err(|_| ApiError::Token(format!("expires_in 格式错误: {text}")))?,
        Value::Number(number) => number
            .as_i64()
            .ok_or_else(|| ApiError::Token(String::from("expires_in 不是整数")))?,
        other => {
            return Err(ApiError::Token(format!("expires_in 类型不支持: {other}")));
        }
    };
    if seconds <= 0 {
        return Err(ApiError::Token(format!("expires_in 必须为正: {seconds}")));
    }
    Ok(seconds as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_margin_matches_go() {
        assert_eq!(EXPIRY_MARGIN, Duration::from_secs(50));
        assert_eq!(FAILURE_COOLDOWN, Duration::from_secs(3));
        assert_eq!(
            DEFAULT_TOKEN_URL,
            "https://bots.qq.com/app/getAppAccessToken"
        );
    }

    #[test]
    fn cached_entry_needs_token_and_future_expiry() {
        let now = Instant::now();
        let empty = Cached::default();
        assert!(!empty.valid(now), "空 token 不算有效");
        let alive = Cached {
            token: String::from("t"),
            expire_at: Some(now + Duration::from_secs(1)),
        };
        assert!(alive.valid(now));
        let dying = Cached {
            token: String::from("t"),
            expire_at: Some(now - Duration::from_secs(1)),
        };
        assert!(!dying.valid(now), "过期 token 不算有效");
        let no_expiry = Cached {
            token: String::from("t"),
            expire_at: None,
        };
        assert!(!no_expiry.valid(now), "没有过期时间不算有效");
    }

    #[test]
    fn expires_in_accepts_string_and_number() {
        let text = serde_json::json!("7200");
        assert_eq!(parse_expires_in(Some(&text)).unwrap(), 7200);
        let number = serde_json::json!(3600);
        assert_eq!(parse_expires_in(Some(&number)).unwrap(), 3600);
        assert!(parse_expires_in(None).is_err());
        assert!(parse_expires_in(Some(&serde_json::json!("abc"))).is_err());
        assert!(parse_expires_in(Some(&serde_json::json!("0"))).is_err());
        assert!(parse_expires_in(Some(&serde_json::json!(true))).is_err());
    }
}
