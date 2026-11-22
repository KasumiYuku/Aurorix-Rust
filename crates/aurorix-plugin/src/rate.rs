use std::sync::Mutex;
use std::time::Duration;

use dashmap::DashMap;
use tokio::time::Instant;

pub const RATE_IDLE_TTL: Duration = Duration::from_secs(600);
pub const RATE_SWEEP_EACH: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rate {
    pub limit: usize,
    pub window: Duration,
}

impl Rate {
    pub fn new(limit: usize, window: Duration) -> Self {
        Self { limit, window }
    }

    pub fn per_window(limit: usize, seconds: u64) -> Self {
        Self {
            limit,
            window: Duration::from_secs(seconds),
        }
    }

    pub fn enabled(&self) -> bool {
        self.limit > 0 && !self.window.is_zero()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RateKey {
    pub command: String,
    pub scene: String,
    pub user: String,
}

impl RateKey {
    pub fn new(
        command: impl Into<String>,
        scene: impl Into<String>,
        user: impl Into<String>,
    ) -> Self {
        Self {
            command: command.into(),
            scene: scene.into(),
            user: user.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct Bucket {
    hits: Vec<Instant>,
    seen: Instant,
}

impl Bucket {
    fn new(seen: Instant) -> Self {
        Self {
            hits: Vec::new(),
            seen,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickRate {
    pub command: String,
    pub rate: Rate,
}

#[derive(Debug)]
pub struct RateLimiter {
    buckets: DashMap<RateKey, Bucket>,
    clicks: DashMap<String, ClickRate>,
    swept_at: Mutex<Instant>,
    max_buckets: usize,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::with_capacity(aurorix_core::HardwareProfile::cached().rate_buckets())
    }

    pub fn with_capacity(max_buckets: usize) -> Self {
        Self {
            buckets: DashMap::new(),
            clicks: DashMap::new(),
            swept_at: Mutex::new(Instant::now()),
            max_buckets: max_buckets.max(1),
        }
    }

    pub fn max_buckets(&self) -> usize {
        self.max_buckets
    }

    pub fn check(&self, key: RateKey, rate: Rate) -> Result<(), Duration> {
        self.check_at(key, rate, Instant::now())
    }

    pub fn check_at(&self, key: RateKey, rate: Rate, now: Instant) -> Result<(), Duration> {
        if !rate.enabled() {
            return Ok(());
        }
        self.sweep(now);
        let mut bucket = self.buckets.entry(key).or_insert_with(|| Bucket::new(now));
        bucket.seen = now;
        bucket
            .hits
            .retain(|at| now.saturating_duration_since(*at) < rate.window);
        if bucket.hits.len() >= rate.limit {
            let wait = match bucket.hits.first() {
                Some(first) => first
                    .checked_add(rate.window)
                    .map(|ready| ready.saturating_duration_since(now))
                    .unwrap_or(Duration::ZERO),
                None => Duration::ZERO,
            };
            return Err(wait);
        }
        bucket.hits.push(now);
        drop(bucket);
        self.enforce_capacity();
        Ok(())
    }

    pub fn wait_seconds(wait: Duration) -> u64 {
        let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
        seconds.max(1)
    }

    pub fn bind_click(
        &self,
        button_id: impl Into<String>,
        command: impl Into<String>,
        rate: Rate,
    ) {
        self.clicks.insert(
            button_id.into(),
            ClickRate {
                command: command.into(),
                rate,
            },
        );
    }

    pub fn click_rate(&self, button_id: &str) -> Option<ClickRate> {
        self.clicks.get(button_id).map(|entry| entry.value().clone())
    }

    pub fn click_allowed(
        &self,
        button_id: &str,
        conversation: &str,
        user: &str,
    ) -> Result<(), Duration> {
        let Some(binding) = self.click_rate(button_id) else {
            return Ok(());
        };
        self.check(
            RateKey::new(binding.command, conversation, user),
            binding.rate,
        )
    }

    pub fn len(&self) -> usize {
        self.buckets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    pub fn clear(&self) {
        self.buckets.clear();
    }

    fn enforce_capacity(&self) {
        while self.buckets.len() > self.max_buckets {
            let mut oldest: Option<(RateKey, Instant)> = None;
            for entry in self.buckets.iter() {
                let seen = entry.value().seen;
                let better = oldest.as_ref().map(|(_, at)| seen < *at).unwrap_or(true);
                if better {
                    oldest = Some((entry.key().clone(), seen));
                }
            }
            let Some((key, _)) = oldest else {
                return;
            };
            self.buckets.remove(&key);
        }
    }

    fn sweep(&self, now: Instant) {
        let mut swept_at = self
            .swept_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if now.saturating_duration_since(*swept_at) < RATE_SWEEP_EACH {
            return;
        }
        *swept_at = now;
        drop(swept_at);
        self.buckets
            .retain(|_, bucket| now.saturating_duration_since(bucket.seen) <= RATE_IDLE_TTL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> RateKey {
        RateKey::new("p song", "group-1", "u1")
    }

    #[test]
    fn unlimited_rate_never_blocks() {
        let limiter = RateLimiter::new();
        let rate = Rate::default();
        assert!(!rate.enabled(), "零值应当不限");
        assert!(limiter.check(key(), rate).is_ok());
        assert!(limiter.check(key(), rate).is_ok());
        assert!(limiter.is_empty(), "不限速时不该建桶");
    }

    #[test]
    fn window_blocks_second_hit_and_reports_wait() {
        let limiter = RateLimiter::new();
        let rate = Rate::per_window(2, 10);
        let now = Instant::now();
        assert!(limiter.check_at(key(), rate, now).is_ok());
        assert!(limiter.check_at(key(), rate, now).is_ok());
        let wait = limiter.check_at(key(), rate, now).unwrap_err();
        assert_eq!(wait, Duration::from_secs(10));
        assert_eq!(RateLimiter::wait_seconds(wait), 10);
    }

    #[test]
    fn expired_hits_are_forgotten() {
        let limiter = RateLimiter::new();
        let rate = Rate::per_window(1, 10);
        let now = Instant::now();
        assert!(limiter.check_at(key(), rate, now).is_ok());
        assert!(limiter
            .check_at(key(), rate, now + Duration::from_secs(9))
            .is_err());
        assert!(
            limiter
                .check_at(key(), rate, now + Duration::from_secs(10))
                .is_ok(),
            "窗口过期后应当放行"
        );
    }

    #[test]
    fn keys_are_isolated_by_command_scene_and_user() {
        let limiter = RateLimiter::new();
        let rate = Rate::per_window(1, 60);
        let now = Instant::now();
        assert!(limiter.check_at(key(), rate, now).is_ok());
        assert!(
            limiter
                .check_at(RateKey::new("p song", "group-2", "u1"), rate, now)
                .is_ok(),
            "换群头一次应当放行"
        );
        assert!(
            limiter
                .check_at(RateKey::new("p song", "group-1", "u2"), rate, now)
                .is_ok(),
            "换用户头一次应当放行"
        );
        assert!(
            limiter
                .check_at(RateKey::new("p b50", "group-1", "u1"), rate, now)
                .is_ok(),
            "换指令头一次应当放行"
        );
        assert_eq!(limiter.len(), 4);
    }

    #[test]
    fn idle_buckets_are_swept() {
        let limiter = RateLimiter::new();
        let rate = Rate::per_window(5, 60);
        let now = Instant::now();
        assert!(limiter.check_at(key(), rate, now).is_ok());
        assert_eq!(limiter.len(), 1);
        let later = now + RATE_IDLE_TTL + RATE_SWEEP_EACH;
        assert!(limiter
            .check_at(RateKey::new("p b50", "group-1", "u9"), rate, later)
            .is_ok());
        assert_eq!(limiter.len(), 1, "空闲桶应当被清扫掉");
    }

    #[test]
    fn wait_seconds_rounds_up() {
        assert_eq!(RateLimiter::wait_seconds(Duration::from_millis(1)), 1);
        assert_eq!(RateLimiter::wait_seconds(Duration::from_millis(1500)), 2);
        assert_eq!(RateLimiter::wait_seconds(Duration::ZERO), 1);
        assert_eq!(RateLimiter::wait_seconds(Duration::from_secs(30)), 30);
    }

    #[test]
    fn clear_drops_every_bucket() {
        let limiter = RateLimiter::new();
        let rate = Rate::per_window(1, 60);
        assert!(limiter.check(key(), rate).is_ok());
        assert!(!limiter.is_empty());
        limiter.clear();
        assert!(limiter.is_empty());
    }

    #[test]
    fn click_binding_shares_the_command_bucket() {
        let limiter = RateLimiter::new();
        let rate = Rate::per_window(2, 10);
        limiter.bind_click("arcaea.score.recent", "arcaea.score recent", rate);
        let binding = limiter.click_rate("arcaea.score.recent").expect("绑定要能读回");
        assert_eq!(binding.command, "arcaea.score recent");
        assert_eq!(binding.rate, rate);
        assert!(limiter
            .click_allowed("arcaea.score.recent", "g1", "u1")
            .is_ok());
        assert!(limiter
            .click_allowed("arcaea.score.recent", "g1", "u1")
            .is_ok());
        assert!(
            limiter
                .click_allowed("arcaea.score.recent", "g1", "u1")
                .is_err(),
            "第三次点击要撞上同一条指令的限流"
        );
        assert!(
            limiter
                .check(RateKey::new("arcaea.score recent", "g1", "u1"), rate)
                .is_err(),
            "点按钮与发指令消耗同一份额度"
        );
        assert!(
            limiter
                .click_allowed("arcaea.score.recent", "g1", "u2")
                .is_ok(),
            "换个人应当放行"
        );
    }

    #[test]
    fn unbound_click_and_zero_rate_stay_free() {
        let limiter = RateLimiter::new();
        assert!(
            limiter.click_allowed("arcaea.absent", "g1", "u1").is_ok(),
            "没绑定的按钮不限速"
        );
        limiter.bind_click("arcaea.zero", "arcaea.zero", Rate::default());
        assert!(limiter.click_allowed("arcaea.zero", "g1", "u1").is_ok());
        assert!(limiter.click_allowed("arcaea.zero", "g1", "u1").is_ok());
        assert!(limiter.is_empty(), "不限速时不该建桶");
    }
}
