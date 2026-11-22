use std::time::Duration;

use rand::Rng;

pub const BASE_DELAY: Duration = Duration::from_millis(1_000);
pub const MAX_DELAY: Duration = Duration::from_millis(30_000);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backoff {
    base: Duration,
    max: Duration,
    current: Duration,
}

impl Backoff {
    pub fn new(base: Duration, max: Duration) -> Self {
        let base = if base.is_zero() {
            Duration::from_millis(1)
        } else {
            base
        };
        let max = if max < base { base } else { max };
        Self {
            base,
            max,
            current: base,
        }
    }

    pub fn default_policy() -> Self {
        Self::new(BASE_DELAY, MAX_DELAY)
    }

    pub fn base(&self) -> Duration {
        self.base
    }

    pub fn max(&self) -> Duration {
        self.max
    }

    pub fn current(&self) -> Duration {
        self.current
    }

    pub fn reset(&mut self) {
        self.current = self.base;
    }

    pub fn advance(&mut self) -> Duration {
        let delay = self.current;
        self.current = double(delay, self.max);
        delay
    }

    pub fn next_delay(&mut self) -> Duration {
        jitter(self.advance())
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::default_policy()
    }
}

pub fn double(current: Duration, max: Duration) -> Duration {
    let doubled = current.saturating_mul(2);
    if doubled >= max {
        max
    } else {
        doubled
    }
}

pub fn jitter(delay: Duration) -> Duration {
    let millis = delay.as_millis();
    let spread = (millis / 2) as i64;
    if spread == 0 {
        return delay;
    }
    let offset = rand::thread_rng().gen_range(0..spread) - spread / 2;
    let shifted = millis as i64 + offset;
    if shifted < 1 {
        Duration::from_millis(1)
    } else {
        Duration::from_millis(shifted as u64)
    }
}
