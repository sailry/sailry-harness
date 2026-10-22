//! One retry budget bounds transport recovery; the final error retains its protected diagnostics.
use super::*;
use adk_core::ErrorCategory;
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct Budget(Mutex<State>);

#[derive(Default)]
struct State {
    timeouts: u8,
    recoveries: u8,
}

impl Budget {
    pub(super) fn schedule(
        &self,
        error: &mut AdkError,
        stopped: bool,
        attempt: u64,
        limit: u32,
        committed: bool,
        published: bool,
    ) -> bool {
        let mut state = self.0.lock().expect("retry budget lock is healthy");
        let long_delay = error.retry.retry_after_ms.filter(|delay| *delay > 60_000);
        let timed_out = error.category == ErrorCategory::Timeout;
        let allowed = scheduled(error, stopped, attempt, limit, committed)
            && long_delay.is_none()
            && (!timed_out || state.timeouts < 2)
            && (!published || state.recoveries < 1);
        error.retry.should_retry &= allowed;
        if allowed {
            state.timeouts += u8::from(timed_out);
            state.recoveries += u8::from(published);
        }
        if let Some(delay) = long_delay {
            error.retry.should_retry = false;
            error
                .details
                .metadata
                .insert("sailry_retry_after_ms".into(), delay.into());
            let explanation =
                format!("\n\nRetry-After is {delay} ms; automatic retry waits at most 60000 ms");
            error.message.push_str(&explanation);
            if let Some(mut fault) = std::error::Error::source(error)
                .and_then(|source| source.downcast_ref::<Fault>())
                .cloned()
            {
                fault.message.push_str(&explanation);
                *error = std::mem::replace(error, cancelled()).with_source(fault);
            }
        }
        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(category: ErrorCategory) -> AdkError {
        AdkError::new(
            adk_core::ErrorComponent::Model,
            category,
            "model.fixture.failure",
            "unchecked body",
        )
    }

    #[test]
    fn shares_sublimits_without_replaying_committed_work() {
        let budget = Budget::default();
        assert!(budget.schedule(
            &mut failure(ErrorCategory::Timeout),
            false,
            1,
            5,
            false,
            false
        ));
        assert!(budget.schedule(
            &mut failure(ErrorCategory::Unavailable),
            false,
            2,
            5,
            false,
            false
        ));
        assert!(budget.schedule(
            &mut failure(ErrorCategory::Timeout),
            false,
            3,
            5,
            false,
            false
        ));
        assert!(!budget.schedule(
            &mut failure(ErrorCategory::Timeout),
            false,
            4,
            5,
            false,
            false
        ));
        assert!(budget.schedule(
            &mut failure(ErrorCategory::Unavailable),
            false,
            4,
            5,
            false,
            true
        ));
        assert!(!budget.schedule(
            &mut failure(ErrorCategory::Unavailable),
            false,
            5,
            5,
            false,
            true
        ));
        assert!(!Budget::default().schedule(
            &mut failure(ErrorCategory::Unavailable),
            false,
            1,
            5,
            true,
            false
        ));
        assert!(!Budget::default().schedule(
            &mut failure(ErrorCategory::Unavailable),
            true,
            1,
            5,
            false,
            false
        ));
        assert!(!Budget::default().schedule(
            &mut failure(ErrorCategory::Unavailable),
            false,
            6,
            5,
            false,
            false
        ));
    }

    #[test]
    fn keeps_only_the_final_protected_cause() {
        let budget = Budget::default();
        let mut first = failure(ErrorCategory::Unavailable)
            .with_upstream_status(503)
            .with_source(Fault::new(
                ErrorCode::Unavailable,
                "Protected early failure",
            ));
        assert!(budget.schedule(&mut first, false, 1, 5, false, false));
        let last = failure(ErrorCategory::RateLimited)
            .with_upstream_status(429)
            .with_source(Fault::new(
                ErrorCode::Unavailable,
                "Protected final failure",
            ));
        let fault = super::super::super::error(last);
        assert_eq!(fault.message, "Protected final failure");
        assert!(!fault.message.contains("Protected early failure"));
        assert!(!fault.message.contains("unchecked body"));
    }

    #[test]
    fn caps_waits_without_repeating_the_final_cause() {
        for protected in [false, true] {
            let budget = Budget::default();
            let mut first = failure(ErrorCategory::Unavailable).with_upstream_status(503);
            assert!(budget.schedule(&mut first, false, 1, 5, false, false));
            let mut last = AdkError::new(
                adk_core::ErrorComponent::Model,
                ErrorCategory::RateLimited,
                "model.fixture.rate_limited",
                "Final provider reason",
            )
            .with_upstream_status(429);
            if protected {
                let fault = Fault::new(
                    ErrorCode::Unavailable,
                    format!(
                        "{}\n\nFinal provider reason",
                        super::super::super::summary(&last)
                    ),
                );
                last = last.with_source(fault);
            }
            last.retry.retry_after_ms = Some(426_414_000);
            assert!(!budget.schedule(&mut last, false, 2, 5, false, false));
            assert_eq!(last.category, ErrorCategory::RateLimited);
            assert_eq!(last.details.upstream_status_code, Some(429));
            assert_eq!(last.retry.retry_after_ms, Some(426_414_000));
            let fault = super::super::super::error(last);
            for text in [
                "Final provider reason",
                "model.fixture.rate_limited",
                "HTTP 429",
                "Retry-After is 426414000 ms",
                "at most 60000 ms",
            ] {
                assert_eq!(fault.message.matches(text).count(), 1);
            }
            assert!(!fault.message.contains("HTTP 503"));
            assert!(!fault.message.contains("First failure"));
            assert!(!fault.message.contains("Final failure"));
        }
    }
}
