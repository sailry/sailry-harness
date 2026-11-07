use std::time::Duration;

pub fn budget() -> Duration {
    // Standard checks elide waits; direct runs can retain the production policy.
    let seconds = if std::env::var("SAILRY_MODEL_SKIP_BACKOFF").is_ok_and(|value| value == "true") {
        40
    } else {
        360
    };
    Duration::from_secs(seconds)
}
