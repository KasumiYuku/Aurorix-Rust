use std::time::Duration;

use crate::error::ProtocolError;

pub const DEFAULT_RETRY_WHEN: [i32; 2] = [11253, 630006];

pub fn should_retry(code: i32, configured: &[i32]) -> bool {
    configured.contains(&code)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(5),
        }
    }
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay,
        }
    }

    pub fn delay_for(&self, attempt: u32) -> Duration {
        let step = attempt.max(1) - 1;
        let factor = 1u32.checked_shl(step).unwrap_or(u32::MAX);
        self.base_delay.saturating_mul(factor).min(self.max_delay)
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Result<Duration, ProtocolError> {
        if attempt == 0 || attempt > self.max_attempts {
            return Err(ProtocolError::RetryExhausted {
                attempts: self.max_attempts,
            });
        }
        Ok(self.delay_for(attempt))
    }

    pub fn attempts(&self) -> u32 {
        self.max_attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_table_matches_go_side() {
        assert_eq!(DEFAULT_RETRY_WHEN, [11253, 630006]);
        assert!(should_retry(11253, &DEFAULT_RETRY_WHEN));
        assert!(should_retry(630006, &DEFAULT_RETRY_WHEN));
        assert!(!should_retry(0, &DEFAULT_RETRY_WHEN));
        assert!(!should_retry(100, &DEFAULT_RETRY_WHEN));
    }

    #[test]
    fn configured_codes_win() {
        let configured = [42, 7];
        assert!(should_retry(42, &configured));
        assert!(should_retry(7, &configured));
        assert!(!should_retry(11253, &configured));
        assert!(!should_retry(42, &[]));
    }

    #[test]
    fn default_policy_is_three_tries_with_backoff() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.base_delay, Duration::from_millis(200));
        assert_eq!(policy.max_delay, Duration::from_secs(5));
        assert_eq!(policy.delay_for(1), Duration::from_millis(200));
        assert_eq!(policy.delay_for(2), Duration::from_millis(400));
        assert_eq!(policy.delay_for(3), Duration::from_millis(800));
    }

    #[test]
    fn backoff_is_monotonic_and_capped() {
        let policy = RetryPolicy::new(64, Duration::from_millis(200), Duration::from_secs(5));
        let mut previous = Duration::ZERO;
        for attempt in 1..=32 {
            let delay = policy.delay_for(attempt);
            assert!(delay >= previous, "第 {attempt} 次退避不该变短");
            assert!(delay <= policy.max_delay, "第 {attempt} 次退避超上限");
            previous = delay;
        }
        assert_eq!(policy.delay_for(32), policy.max_delay);
        assert_eq!(policy.delay_for(u32::MAX), policy.max_delay);
    }

    #[test]
    fn attempts_beyond_limit_are_rejected() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.delay_for_attempt(1), Ok(Duration::from_millis(200)));
        assert_eq!(policy.delay_for_attempt(3), Ok(Duration::from_millis(800)));
        assert_eq!(
            policy.delay_for_attempt(4),
            Err(ProtocolError::RetryExhausted { attempts: 3 }),
            "超过上限要报错"
        );
        assert_eq!(
            policy.delay_for_attempt(0),
            Err(ProtocolError::RetryExhausted { attempts: 3 })
        );
        assert_eq!(
            policy.delay_for_attempt(4).unwrap_err().to_string(),
            "重试次数已用尽, 上限 3 次"
        );
        assert_eq!(policy.attempts(), 3);
    }
}
