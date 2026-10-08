//! Exponential backoff with jitter and Retry-After support.

use crate::config::RetryConfig;
use rand::Rng;
use std::time::Duration;

pub struct Backoff {
    config: RetryConfig,
    attempt: u32,
}

impl Backoff {
    pub fn new(config: RetryConfig) -> Self {
        Self { config, attempt: 0 }
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Returns None if max retries exceeded.
    pub fn next_delay(&mut self) -> Option<Duration> {
        if let Some(max) = self.config.max_retries {
            if self.attempt >= max {
                return None;
            }
        }
        let base = self.config.initial_delay_ms as f64;
        let exp = base * 2f64.powi(self.attempt as i32);
        let mut ms = exp.min((self.config.max_delay_secs as f64) * 1000.0) as u64;
        if self.config.jitter {
            let mut rng = rand::thread_rng();
            let jitter = rng.gen_range(0..=ms / 4);
            ms = ms.saturating_add(jitter);
        }
        self.attempt += 1;
        Some(Duration::from_millis(ms))
    }

    pub fn next_delay_with_retry_after(&mut self, retry_after: Option<u64>) -> Option<Duration> {
        if let Some(secs) = retry_after {
            if let Some(max) = self.config.max_retries {
                if self.attempt >= max {
                    return None;
                }
            }
            self.attempt += 1;
            let capped = secs.min(self.config.max_delay_secs * 2);
            return Some(Duration::from_secs(capped));
        }
        self.next_delay()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_increases() {
        let cfg = RetryConfig {
            max_retries: Some(5),
            initial_delay_ms: 1000,
            max_delay_secs: 60,
            jitter: false,
        };
        let mut b = Backoff::new(cfg);
        let d1 = b.next_delay().unwrap();
        let d2 = b.next_delay().unwrap();
        assert!(d2 >= d1);
    }

    #[test]
    fn backoff_respects_max() {
        let cfg = RetryConfig {
            max_retries: Some(2),
            initial_delay_ms: 100,
            max_delay_secs: 10,
            jitter: false,
        };
        let mut b = Backoff::new(cfg);
        assert!(b.next_delay().is_some());
        assert!(b.next_delay().is_some());
        assert!(b.next_delay().is_none());
    }
}
