use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::oneshot;

use crate::error::ApiError;

pub const AUDIT_TIMEOUT: Duration = Duration::from_secs(30);
pub const AUDIT_PENDING_CODE: i32 = 304023;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditOutcome {
    pub approved: bool,
    pub message_id: String,
}

pub struct AuditRegistry {
    waiters: DashMap<String, oneshot::Sender<AuditOutcome>>,
    timeout_ms: AtomicU64,
}

impl AuditRegistry {
    pub fn new() -> Self {
        Self {
            waiters: DashMap::new(),
            timeout_ms: AtomicU64::new(AUDIT_TIMEOUT.as_millis() as u64),
        }
    }

    pub fn set_timeout(&self, timeout: Duration) {
        self.timeout_ms
            .store(timeout.as_millis() as u64, Ordering::SeqCst);
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.load(Ordering::SeqCst))
    }

    pub fn resolve(&self, audit_id: &str, message_id: &str, approved: bool) -> bool {
        let waiter = self.waiters.remove(audit_id).map(|(_, sender)| sender);
        match waiter {
            Some(sender) => sender
                .send(AuditOutcome {
                    approved,
                    message_id: message_id.to_string(),
                })
                .is_ok(),
            None => false,
        }
    }

    pub fn waiting(&self) -> usize {
        self.waiters.len()
    }

    pub async fn wait(&self, audit_id: &str) -> Result<AuditOutcome, ApiError> {
        let (sender, receiver) = oneshot::channel();
        self.waiters.insert(audit_id.to_string(), sender);
        let timeout = self.timeout();
        let outcome = tokio::time::timeout(timeout, receiver).await;
        self.waiters.remove(audit_id);
        match outcome {
            Err(_) => Err(ApiError::AuditTimeout),
            Ok(Err(_)) => Err(ApiError::AuditTimeout),
            Ok(Ok(outcome)) => {
                if outcome.approved {
                    Ok(outcome)
                } else {
                    Err(ApiError::AuditRejected)
                }
            }
        }
    }
}

impl Default for AuditRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_is_thirty_seconds() {
        assert_eq!(AUDIT_TIMEOUT, Duration::from_secs(30));
        assert_eq!(AuditRegistry::new().timeout(), Duration::from_secs(30));
        assert_eq!(AUDIT_PENDING_CODE, 304023);
    }

    #[tokio::test]
    async fn approved_resolution_returns_outcome() {
        let registry = std::sync::Arc::new(AuditRegistry::new());
        let waiter = {
            let clone = std::sync::Arc::clone(&registry);
            tokio::spawn(async move { clone.wait("a1").await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(registry.resolve("a1", "m1", true), "等待者要被唤醒");
        let outcome = waiter.await.unwrap().unwrap();
        assert_eq!(outcome.message_id, "m1");
        assert!(outcome.approved);
        assert!(!registry.resolve("a1", "m1", true), "重复回执不该命中");
    }

    #[tokio::test]
    async fn rejected_audit_reports_failure() {
        let registry = std::sync::Arc::new(AuditRegistry::new());
        let waiter = {
            let clone = std::sync::Arc::clone(&registry);
            tokio::spawn(async move { clone.wait("a2").await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        registry.resolve("a2", "", false);
        assert_eq!(waiter.await.unwrap(), Err(ApiError::AuditRejected));
    }

    #[tokio::test]
    async fn silent_audit_times_out() {
        let registry = std::sync::Arc::new(AuditRegistry::new());
        registry.set_timeout(Duration::from_millis(80));
        let result = registry.wait("a3").await;
        assert_eq!(result, Err(ApiError::AuditTimeout));
        assert_eq!(registry.waiting(), 0, "超时后不能残留等待者");
    }
}
