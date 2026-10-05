use std::time::Duration;

pub fn budget() -> Duration {
    // Keep the ordinary completion allowance separate from ADK's five backoffs.
    let policy = adk_model::retry::RetryConfig::default().with_max_retries(5);
    let mut delay = policy.initial_delay;
    let mut budget = Duration::from_secs(10);
    for _ in 0..policy.max_retries {
        budget += delay;
        delay = delay
            .mul_f64(f64::from(policy.backoff_multiplier.max(1.0)))
            .min(policy.max_delay);
    }
    budget
}
