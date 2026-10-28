//! Bounded observation never turns durable child admission into an execution timeout.
use super::*;
use std::future::Future;

// Leave the remainder of ADK's tool budget for package VMs and Node settlement.
const WINDOW: Duration = Duration::from_secs(150);

pub(super) struct Observer {
    stop: CancellationToken,
    detached: bool,
}

impl Observer {
    pub fn new(stop: CancellationToken, detached: bool) -> Self {
        Self { stop, detached }
    }
    pub fn detach(&mut self) {
        self.detached = true;
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        if !self.detached {
            self.stop.cancel();
        }
    }
}

pub(super) fn window(controls: &super::super::Controls) -> Duration {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(window) = *controls.child_observation.lock().unwrap() {
        return window;
    }
    #[cfg(not(any(test, feature = "test-support")))]
    let _ = controls;
    WINDOW
}

pub(super) async fn wait<T>(
    future: impl Future<Output = T>,
    stop: &CancellationToken,
    window: Duration,
) -> Result<Option<T>, Fault> {
    tokio::pin!(future);
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "child observation was cancelled")),
        value = &mut future => Ok(Some(value)),
        _ = tokio::time::sleep(window) => Ok(None),
    }
}

pub(super) fn pending(run: &sailry_protocol::conversation::Run) -> Value {
    json!({"admitted":true,"pending":true,"session":run.session,"turn":run.turn,"status":run.status})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn expiry_preserves_owned_work() {
        let stop = CancellationToken::new();
        let mut observer = Observer::new(stop.clone(), false);
        assert_eq!(
            wait(
                std::future::pending::<()>(),
                &CancellationToken::new(),
                Duration::ZERO
            )
            .await
            .unwrap(),
            None
        );
        observer.detach();
        drop(observer);
        assert!(!stop.is_cancelled());
    }

    #[test]
    fn cancellation_keeps_cleanup_ownership() {
        let stop = CancellationToken::new();
        drop(Observer::new(stop.clone(), false));
        assert!(stop.is_cancelled());
    }

    #[tokio::test]
    async fn completed_work_returns_before_expiry() {
        assert_eq!(
            wait(async { 7 }, &CancellationToken::new(), Duration::ZERO)
                .await
                .unwrap(),
            Some(7)
        );
        let stop = CancellationToken::new();
        stop.cancel();
        assert_eq!(
            wait(std::future::pending::<()>(), &stop, Duration::ZERO)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
    }
}
