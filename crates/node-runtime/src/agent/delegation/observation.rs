//! Bounded observation never turns durable child admission into an execution timeout.
use super::*;

// Status observation returns pending while durable child work keeps its owner.
const WINDOW: Duration = Duration::from_secs(150);

pub(super) fn window(controls: &super::super::Controls) -> Duration {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(window) = *controls.child_observation.lock().unwrap() {
        return window;
    }
    #[cfg(not(any(test, feature = "test-support")))]
    let _ = controls;
    WINDOW
}

pub(super) fn pending(run: &sailry_protocol::conversation::Run) -> Value {
    json!({"admitted":true,"pending":true,"session":run.session,"turn":run.turn,"status":run.status})
}
