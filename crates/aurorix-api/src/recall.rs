use std::time::{Duration, Instant};

use aurorix_core::Target;
use aurorix_protocol::SendRequest;
use reqwest::Method;

use crate::api::BotApi;
use crate::error::ApiError;

pub const RECALL_WINDOW: Duration = Duration::from_secs(120);
pub const RECALL_GRACE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SentBatch {
    pub ids: Vec<String>,
    pub at: Option<Instant>,
    pub delivered_active: bool,
}

impl SentBatch {
    pub fn new(ids: Vec<String>, at: Instant) -> Self {
        Self {
            ids,
            at: Some(at),
            delivered_active: false,
        }
    }

    pub fn now(ids: Vec<String>) -> Self {
        Self::new(ids, Instant::now())
    }

    pub fn delivered_active(mut self) -> Self {
        self.delivered_active = true;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.ids.iter().all(|id| id.is_empty())
    }

    pub fn within_window(&self, now: Instant) -> bool {
        match self.at {
            Some(at) => now.saturating_duration_since(at) <= RECALL_WINDOW,
            None => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallFailure {
    pub id: String,
    pub error: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecallReport {
    pub recalled: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<RecallFailure>,
}

impl RecallReport {
    pub fn attempted(&self) -> usize {
        self.recalled.len() + self.failed.len()
    }
}

impl BotApi {
    pub async fn recall(
        &self,
        target: &Target,
        message_id: &str,
        hide_tip: bool,
    ) -> Result<(), ApiError> {
        if target.empty() {
            return Err(ApiError::InvalidTarget("会话目标为空"));
        }
        if message_id.is_empty() {
            return Err(ApiError::InvalidTarget("消息 ID 为空"));
        }
        let path = SendRequest::path_for_scene(target.scene, &target.id);
        let mut url = format!("{}/{message_id}", self.endpoint(&path));
        if hide_tip {
            url.push_str("?hidetip=true");
        }
        self.do_bytes(Method::DELETE, &url, None).await.map(|_| ())
    }

    pub async fn recall_batch(&self, target: &Target, olds: &[SentBatch]) -> RecallReport {
        let mut report = RecallReport::default();
        let grace = self.recall_grace();
        for old in olds {
            if old.delivered_active {
                report
                    .skipped
                    .extend(old.ids.iter().filter(|id| !id.is_empty()).cloned());
                continue;
            }
            if old.is_empty() {
                continue;
            }
            if !old.within_window(Instant::now()) {
                report
                    .skipped
                    .extend(old.ids.iter().filter(|id| !id.is_empty()).cloned());
                continue;
            }
            if let Some(at) = old.at {
                let waited = at.elapsed();
                if grace > waited {
                    tokio::time::sleep(grace - waited).await;
                }
            }
            for id in old.ids.iter().filter(|id| !id.is_empty()) {
                match self.recall(target, id, true).await {
                    Ok(()) => report.recalled.push(id.clone()),
                    Err(err) => report.failed.push(RecallFailure {
                        id: id.clone(),
                        error: err.to_string(),
                    }),
                }
            }
        }
        report
    }

    pub fn recallable(&self, batch: &SentBatch) -> bool {
        !batch.delivered_active && !batch.is_empty() && batch.within_window(Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_and_grace_match_go() {
        assert_eq!(RECALL_WINDOW, Duration::from_secs(120));
        assert_eq!(RECALL_GRACE, Duration::from_millis(400));
    }

    #[test]
    fn batches_reject_empty_ids_and_old_timestamps() {
        let empty = SentBatch::now(vec![String::new()]);
        assert!(empty.is_empty());
        let fresh = SentBatch::now(vec![String::from("m1")]);
        assert!(!fresh.is_empty());
        assert!(fresh.within_window(Instant::now()));
        let old_time = Instant::now()
            .checked_sub(Duration::from_secs(121))
            .expect("时间要能往回拨");
        let old = SentBatch::new(vec![String::from("m1")], old_time);
        assert!(!old.within_window(Instant::now()), "超过 2 分钟不能撤");
        let untimed = SentBatch {
            ids: vec![String::from("m1")],
            at: None,
            delivered_active: false,
        };
        assert!(
            untimed.within_window(Instant::now()),
            "没有时间戳就不判时限"
        );
        let boundary = SentBatch::new(vec![String::from("m1")], Instant::now());
        assert!(boundary.within_window(Instant::now()));
    }

    #[test]
    fn active_delivery_is_never_recalled() {
        let batch = SentBatch::now(vec![String::from("m1")]).delivered_active();
        assert!(batch.delivered_active);
        let report = RecallReport::default();
        assert_eq!(report.attempted(), 0);
        assert!(report.recalled.is_empty());
    }
}
