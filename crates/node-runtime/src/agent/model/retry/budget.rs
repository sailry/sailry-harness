//! One retry budget bounds transport recovery and retains protected diagnostics.
use super::*;
use adk_core::ErrorCategory;
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct Budget(Mutex<State>);

#[derive(Default)]
struct State {
    timeouts: u8,
    recoveries: u8,
    first: Option<Cause>,
}

#[derive(Clone)]
struct Cause {
    code: &'static str,
    status: Option<u16>,
    fault: Fault,
}

impl Cause {
    fn new(error: &AdkError) -> Self {
        Self {
            code: error.code,
            status: error.details.upstream_status_code,
            fault: std::error::Error::source(error)
                .and_then(|source| source.downcast_ref::<Fault>())
                .cloned()
                .unwrap_or_else(|| {
                    Fault::new(ErrorCode::Unavailable, super::super::summary(error))
                }),
        }
    }

    fn label(&self) -> String {
        match self.status {
            Some(status) => format!("{}; HTTP {status}", self.code),
            None => self.code.into(),
        }
    }
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
        if state.first.is_none() {
            state.first = Some(Cause::new(error));
        }
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
            let cause = Cause::new(error);
            error.retry.should_retry = false;
            error
                .details
                .metadata
                .insert("sailry_retry_after_ms".into(), delay.into());
            *error = std::mem::replace(error, cancelled()).with_source(Fault::new(
                cause.fault.code,
                format!(
                    "{}\n\nRetry-After is {delay} ms; automatic retry waits at most 60000 ms",
                    cause.fault.message
                ),
            ));
        }
        allowed
    }

    pub(super) fn finish(&self, error: AdkError) -> AdkError {
        let state = self.0.lock().expect("retry budget lock is healthy");
        let Some(first) = &state.first else {
            return error;
        };
        let last = Cause::new(&error);
        if first.code == last.code && first.status == last.status && first.fault == last.fault {
            return error;
        }
        let fault = Fault::new(
            last.fault.code,
            format!(
                "First failure ({}):\n{}\n\nFinal failure ({}):\n{}",
                first.label(),
                first.fault.message,
                last.label(),
                last.fault.message,
            ),
        );
        error.with_source(fault)
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
    fn retains_protected_first_and_final_causes() {
        let budget = Budget::default();
        let mut first = failure(ErrorCategory::Unavailable)
            .with_upstream_status(503)
            .with_source(Fault::new(
                ErrorCode::Unavailable,
                "Upstream original message",
            ));
        assert!(budget.schedule(&mut first, false, 1, 5, false, false));
        let last = failure(ErrorCategory::RateLimited)
            .with_upstream_status(429)
            .with_source(Fault::new(ErrorCode::Unavailable, "Final upstream message"));
        let fault = super::super::super::error(budget.finish(last));
        assert!(fault.message.contains("Upstream original message"));
        assert!(fault.message.contains("Final upstream message"));
        assert!(fault.message.contains("HTTP 503"));
        assert!(fault.message.contains("HTTP 429"));
        assert!(!fault.message.contains("unchecked body"));
    }
}
