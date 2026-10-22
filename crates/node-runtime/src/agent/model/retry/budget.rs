//! Session recovery follows openai/codex 61f34c35 core/responses_retry.rs.
//! HTTP request retries belong to the transport; these counters never own them.
use super::classification::{Class, Failure};
use sailry_protocol::conversation::recovery::Stop;
use std::time::Duration;
use tokio::time::Instant;

#[derive(Default)]
pub(super) struct Budget {
    retries: u64,
    connections: u64,
}

#[derive(Debug)]
pub(super) enum Decision {
    Retry {
        attempt: u64,
        limit: Option<u64>,
        at: Instant,
        delay: Option<Duration>,
    },
    Exhausted,
    Stop(Stop),
}

impl Budget {
    pub fn next(&mut self, failure: &Failure, limit: u64, persistent: bool) -> Decision {
        match failure.class {
            Class::Terminal(reason) => return Decision::Stop(reason),
            Class::Advice(reason) if failure.retry_at.is_none() => return Decision::Stop(reason),
            _ => {}
        }
        if failure.class == Class::Connection && persistent {
            self.connections = self.connections.saturating_add(1);
            let seconds = 5_u64
                .saturating_mul(
                    2_u64.saturating_pow(self.connections.saturating_sub(1).min(4) as u32),
                )
                .min(60);
            return Decision::Retry {
                attempt: self.connections,
                limit: None,
                at: Instant::now() + Duration::from_secs(seconds),
                delay: Some(Duration::from_secs(seconds)),
            };
        }
        if self.retries >= limit {
            return Decision::Exhausted;
        }
        self.retries += 1;
        let delay = failure.retry_at.is_none().then(|| backoff(self.retries));
        Decision::Retry {
            attempt: self.retries,
            limit: Some(limit),
            at: failure
                .retry_at
                .unwrap_or_else(|| Instant::now() + delay.expect("local backoff")),
            delay,
        }
    }

    /// A transport switch resets only the finite counter, like Codex.
    pub fn fallback(&mut self) {
        self.retries = 0;
    }
}

pub(in crate::agent::model) fn backoff(attempt: u64) -> Duration {
    // Existing UUID entropy avoids a separate application RNG owner.
    let entropy = uuid::Uuid::new_v4();
    let sample = u32::from_le_bytes(entropy.as_bytes()[..4].try_into().expect("four bytes"));
    let jitter = 0.9 + 0.2 * f64::from(sample) / (f64::from(u32::MAX) + 1.0);
    let base = (200.0 * 2.0_f64.powi(attempt.saturating_sub(1) as i32)) as u64;
    Duration::from_millis((base as f64 * jitter) as u64)
}

#[cfg(test)]
mod tests;
