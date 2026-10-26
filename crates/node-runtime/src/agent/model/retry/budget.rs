//! One budget covers every recovery of an unfinished model request.
use super::classification::{Class, Failure};
use sailry_protocol::conversation::recovery::Stop;
use std::time::Duration;
use tokio::time::Instant;

pub(super) const INTERVAL: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct Budget {
    retries: u64,
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
    pub fn next(&mut self, failure: &Failure, limit: u64) -> Decision {
        if let Class::Terminal(reason) = failure.class {
            return Decision::Stop(reason);
        }
        if self.retries >= limit {
            return Decision::Exhausted;
        }
        self.retries += 1;
        let minimum = Instant::now() + INTERVAL;
        let at = failure.retry_at.unwrap_or(minimum).max(minimum);
        Decision::Retry {
            attempt: self.retries,
            limit: Some(limit),
            at,
            delay: (at == minimum).then_some(INTERVAL),
        }
    }
}

#[cfg(test)]
mod tests;
