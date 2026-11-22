use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

pub const DEFAULT_FAIL_THRESHOLD: u32 = 3;

pub const DEFAULT_COOLDOWN_MIN: Duration = Duration::from_secs(60);

pub const DEFAULT_COOLDOWN_MAX: Duration = Duration::from_secs(120);

pub trait BreakerClock: Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl BreakerClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerConfig {
    pub fail_threshold: u32,
    pub cooldown_min: Duration,
    pub cooldown_max: Duration,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            fail_threshold: DEFAULT_FAIL_THRESHOLD,
            cooldown_min: DEFAULT_COOLDOWN_MIN,
            cooldown_max: DEFAULT_COOLDOWN_MAX,
        }
    }
}

impl BreakerConfig {
    pub fn new(fail_threshold: u32, cooldown_min: Duration, cooldown_max: Duration) -> Self {
        let min = if cooldown_min.is_zero() {
            DEFAULT_COOLDOWN_MIN
        } else {
            cooldown_min
        };
        let max = if cooldown_max < min {
            min
        } else {
            cooldown_max
        };
        Self {
            fail_threshold: fail_threshold.max(1),
            cooldown_min: min,
            cooldown_max: max,
        }
    }

    pub fn with_fail_threshold(mut self, fail_threshold: u32) -> Self {
        self.fail_threshold = fail_threshold.max(1);
        self
    }

    pub fn with_cooldowns(mut self, cooldown_min: Duration, cooldown_max: Duration) -> Self {
        let min = if cooldown_min.is_zero() {
            DEFAULT_COOLDOWN_MIN
        } else {
            cooldown_min
        };
        self.cooldown_min = min;
        self.cooldown_max = if cooldown_max < min {
            min
        } else {
            cooldown_max
        };
        self
    }

    pub fn cooldown_for(&self, open_count: u64) -> Duration {
        let doublings = open_count.saturating_sub(1).min(8) as u32;
        let scaled = self
            .cooldown_min
            .checked_mul(1u32 << doublings)
            .unwrap_or(self.cooldown_max);
        if scaled > self.cooldown_max {
            self.cooldown_max
        } else {
            scaled
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerVerdict {
    Allow,
    Probe,
    Blocked,
}

impl BreakerVerdict {
    pub fn blocked(self) -> bool {
        matches!(self, Self::Blocked)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerEvent {
    Closed,
    Streak { streak: u32 },
    Opened { cooldown: Duration, open_count: u64 },
    AlreadyOpen,
}

impl BreakerEvent {
    pub fn opened(self) -> bool {
        matches!(self, Self::Opened { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakerSnapshot {
    pub provider: String,
    pub fail_streak: u32,
    pub open: bool,
    pub open_count: u64,
    pub skips: u64,
}

#[derive(Debug, Clone, Default)]
struct Entry {
    fail_streak: u32,
    open_until: Option<Instant>,
    open_count: u64,
    skips: u64,
    probing: bool,
}

impl Entry {
    fn closed(&mut self) {
        self.fail_streak = 0;
        self.open_until = None;
        self.probing = false;
    }
}

pub struct CircuitBreaker {
    config: BreakerConfig,
    clock: Arc<dyn BreakerClock>,
    entries: Mutex<BTreeMap<String, Entry>>,
}

impl std::fmt::Debug for CircuitBreaker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CircuitBreaker")
            .field("config", &self.config)
            .field("tracked", &self.entries.lock().len())
            .finish()
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(BreakerConfig::default())
    }
}

impl CircuitBreaker {
    pub fn new(config: BreakerConfig) -> Self {
        Self::with_clock(config, Arc::new(SystemClock))
    }

    pub fn with_clock(config: BreakerConfig, clock: Arc<dyn BreakerClock>) -> Self {
        Self {
            config,
            clock,
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn config(&self) -> BreakerConfig {
        self.config
    }

    pub fn verdict(&self, provider: &str) -> BreakerVerdict {
        let now = self.clock.now();
        let mut entries = self.entries.lock();
        let entry = entries.entry(provider.to_string()).or_default();
        match entry.open_until {
            None => BreakerVerdict::Allow,
            Some(until) if now < until => {
                entry.skips = entry.skips.saturating_add(1);
                BreakerVerdict::Blocked
            }
            Some(_) => {
                entry.probing = true;
                BreakerVerdict::Probe
            }
        }
    }

    pub fn is_open(&self, provider: &str) -> bool {
        let now = self.clock.now();
        let entries = self.entries.lock();
        entries
            .get(provider)
            .and_then(|entry| entry.open_until)
            .is_some_and(|until| now < until)
    }

    pub fn record_success(&self, provider: &str) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(provider) {
            entry.closed();
        }
    }

    pub fn record_failure(&self, provider: &str) -> BreakerEvent {
        let now = self.clock.now();
        let mut entries = self.entries.lock();
        let entry = entries.entry(provider.to_string()).or_default();
        entry.probing = false;
        entry.fail_streak = entry.fail_streak.saturating_add(1);
        if entry.open_until.is_some() {
            let cooldown = self.config.cooldown_for(entry.open_count.saturating_add(1));
            entry.open_count = entry.open_count.saturating_add(1);
            entry.open_until = Some(now + cooldown);
            return BreakerEvent::Opened {
                cooldown,
                open_count: entry.open_count,
            };
        }
        if entry.fail_streak < self.config.fail_threshold {
            return BreakerEvent::Streak {
                streak: entry.fail_streak,
            };
        }
        let cooldown = self.config.cooldown_for(entry.open_count.saturating_add(1));
        entry.open_count = entry.open_count.saturating_add(1);
        entry.open_until = Some(now + cooldown);
        BreakerEvent::Opened {
            cooldown,
            open_count: entry.open_count,
        }
    }

    pub fn reset(&self, provider: &str) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(provider) {
            *entry = Entry::default();
        }
    }

    pub fn reset_all(&self) {
        self.entries.lock().clear();
    }

    pub fn fail_streak(&self, provider: &str) -> u32 {
        self.entries
            .lock()
            .get(provider)
            .map(|entry| entry.fail_streak)
            .unwrap_or(0)
    }

    pub fn snapshots(&self) -> Vec<BreakerSnapshot> {
        let now = self.clock.now();
        self.entries
            .lock()
            .iter()
            .map(|(provider, entry)| BreakerSnapshot {
                provider: provider.clone(),
                fail_streak: entry.fail_streak,
                open: entry.open_until.is_some_and(|until| now < until),
                open_count: entry.open_count,
                skips: entry.skips,
            })
            .collect()
    }

    pub fn open_providers(&self) -> Vec<String> {
        let now = self.clock.now();
        self.entries
            .lock()
            .iter()
            .filter(|(_, entry)| entry.open_until.is_some_and(|until| now < until))
            .map(|(provider, _)| provider.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::FakeClock;

    fn breaker(clock: &Arc<FakeClock>) -> CircuitBreaker {
        CircuitBreaker::with_clock(
            BreakerConfig::new(3, Duration::from_secs(60), Duration::from_secs(120)),
            clock.clone(),
        )
    }

    fn open(breaker: &CircuitBreaker) {
        for _ in 0..3 {
            breaker.record_failure("sgame");
        }
    }

    #[test]
    fn third_failure_opens_the_circuit() {
        let clock = FakeClock::shared();
        let breaker = breaker(&clock);
        for expected in 1..=2 {
            match breaker.record_failure("sgame") {
                BreakerEvent::Streak { streak } => assert_eq!(streak, expected),
                other => panic!("应当只是连续失败计数, 实际 {other:?}"),
            }
        }
        match breaker.record_failure("sgame") {
            BreakerEvent::Opened {
                cooldown,
                open_count,
            } => {
                assert_eq!(cooldown, Duration::from_secs(60));
                assert_eq!(open_count, 1);
            }
            other => panic!("第三次失败应当熔断, 实际 {other:?}"),
        }
        assert!(breaker.is_open("sgame"));
        assert_eq!(breaker.verdict("sgame"), BreakerVerdict::Blocked);
        assert_eq!(breaker.open_providers(), vec!["sgame".to_string()]);
    }

    #[test]
    fn cooldown_expiry_yields_probe_and_success_closes() {
        let clock = FakeClock::shared();
        let breaker = breaker(&clock);
        open(&breaker);
        clock.advance(Duration::from_secs(59));
        assert_eq!(breaker.verdict("sgame"), BreakerVerdict::Blocked);
        clock.advance(Duration::from_secs(2));
        assert_eq!(breaker.verdict("sgame"), BreakerVerdict::Probe);
        assert_eq!(breaker.verdict("sgame"), BreakerVerdict::Probe);
        breaker.record_success("sgame");
        assert_eq!(breaker.verdict("sgame"), BreakerVerdict::Allow);
        assert_eq!(breaker.fail_streak("sgame"), 0);
        assert!(breaker.open_providers().is_empty());
    }

    #[test]
    fn probe_failure_doubles_cooldown_and_caps_at_max() {
        let clock = FakeClock::shared();
        let breaker = breaker(&clock);
        open(&breaker);
        clock.advance(Duration::from_secs(61));
        assert_eq!(breaker.verdict("sgame"), BreakerVerdict::Probe);
        match breaker.record_failure("sgame") {
            BreakerEvent::Opened {
                cooldown,
                open_count,
            } => {
                assert_eq!(cooldown, Duration::from_secs(120));
                assert_eq!(open_count, 2);
            }
            other => panic!("探测失败应当延长冷却, 实际 {other:?}"),
        }
        clock.advance(Duration::from_secs(121));
        assert_eq!(breaker.verdict("sgame"), BreakerVerdict::Probe);
        match breaker.record_failure("sgame") {
            BreakerEvent::Opened {
                cooldown,
                open_count,
            } => {
                assert_eq!(cooldown, Duration::from_secs(120));
                assert_eq!(open_count, 3);
            }
            other => panic!("冷却上限应当封顶, 实际 {other:?}"),
        }
    }

    #[test]
    fn success_resets_the_streak_before_the_threshold() {
        let clock = FakeClock::shared();
        let breaker = breaker(&clock);
        breaker.record_failure("sgame");
        breaker.record_failure("sgame");
        breaker.record_success("sgame");
        assert_eq!(breaker.fail_streak("sgame"), 0);
        match breaker.record_failure("sgame") {
            BreakerEvent::Streak { streak } => assert_eq!(streak, 1),
            other => panic!("成功后应当重新计数, 实际 {other:?}"),
        }
    }

    #[test]
    fn snapshots_report_open_state_and_skip_count() {
        let clock = FakeClock::shared();
        let breaker = breaker(&clock);
        open(&breaker);
        breaker.verdict("sgame");
        breaker.verdict("sgame");
        let snapshots = breaker.snapshots();
        assert_eq!(snapshots.len(), 1);
        let snapshot = &snapshots[0];
        assert_eq!(snapshot.provider, "sgame");
        assert_eq!(snapshot.fail_streak, 3);
        assert!(snapshot.open);
        assert_eq!(snapshot.open_count, 1);
        assert_eq!(snapshot.skips, 2);
    }

    #[test]
    fn reset_clears_the_open_state() {
        let clock = FakeClock::shared();
        let breaker = breaker(&clock);
        open(&breaker);
        breaker.reset("sgame");
        assert_eq!(breaker.verdict("sgame"), BreakerVerdict::Allow);
        assert_eq!(breaker.fail_streak("sgame"), 0);
        open(&breaker);
        breaker.reset_all();
        assert!(breaker.snapshots().is_empty());
    }

    #[test]
    fn config_clamps_threshold_and_cooldowns() {
        let config = BreakerConfig::new(0, Duration::ZERO, Duration::from_secs(1));
        assert_eq!(config.fail_threshold, 1);
        assert_eq!(config.cooldown_min, DEFAULT_COOLDOWN_MIN);
        assert_eq!(config.cooldown_max, DEFAULT_COOLDOWN_MIN);
        let config = BreakerConfig::default();
        assert_eq!(config.cooldown_for(1), DEFAULT_COOLDOWN_MIN);
        assert_eq!(config.cooldown_for(2), DEFAULT_COOLDOWN_MAX);
        assert_eq!(config.cooldown_for(9), DEFAULT_COOLDOWN_MAX);
        assert_eq!(config.cooldown_for(0), DEFAULT_COOLDOWN_MIN);
    }
}
