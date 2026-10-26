use std::time::Duration;

pub fn budget() -> Duration {
    // Service fixtures elide recovery waits; paused-time unit checks cover timing.
    Duration::from_secs(40)
}
