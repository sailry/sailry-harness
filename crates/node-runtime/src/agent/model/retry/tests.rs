use super::*;

mod retry_diagnostics {
    use super::*;

    #[test]
    fn excludes_nonretryable_and_exhausted() {
        let nonretryable = AdkError::new(
            adk_core::ErrorComponent::Model,
            adk_core::ErrorCategory::Internal,
            "model.fixture.invalid",
            "fixture",
        );
        let retryable = AdkError::new(
            adk_core::ErrorComponent::Model,
            adk_core::ErrorCategory::Unavailable,
            "model.fixture.unavailable",
            "fixture",
        );
        assert!(!scheduled(&nonretryable, false, 1, 5, false));
        assert!(scheduled(&retryable, false, 5, 5, false));
        assert!(!scheduled(&retryable, false, 6, 5, false));
        assert!(!scheduled(&retryable, true, 1, 5, false));
        assert!(!scheduled(&retryable, false, 1, 5, true));
    }
}

#[tokio::test(start_paused = true)]
async fn bounds_timeout_retries_within_total_budget() {
    let budget = budget::Budget::default();
    let mut attempts = 0;
    let result = execute_with_retry_observer(
        &RetryConfig::default().with_max_retries(5),
        retryable,
        None,
        &mut || {
            attempts += 1;
            let attempt = attempts;
            let budget = &budget;
            async move {
                let mut error = AdkError::new(
                    adk_core::ErrorComponent::Model,
                    adk_core::ErrorCategory::Timeout,
                    "model.fixture.timeout",
                    "transport idle timeout",
                );
                budget.schedule(&mut error, false, attempt, 5, false, false);
                Err::<(), _>(error)
            }
        },
        |_, _, _| async { Ok(()) },
    )
    .await;
    assert_eq!(result.err().unwrap().code, "model.fixture.timeout");
    assert_eq!(attempts, 3);
}

#[tokio::test(start_paused = true)]
async fn honors_bounded_server_delays() {
    for seconds in [2, 60, 61, 86_400] {
        let budget = budget::Budget::default();
        let mut attempts = 0;
        let mut observed = Vec::new();
        let result = execute_with_retry_observer(
            &RetryConfig::default()
                .with_max_retries(5)
                .with_max_delay(std::time::Duration::from_secs(60)),
            retryable,
            None,
            &mut || {
                attempts += 1;
                let mut error = AdkError::new(
                    adk_core::ErrorComponent::Model,
                    adk_core::ErrorCategory::RateLimited,
                    "model.fixture.rate_limit",
                    "rate limited",
                );
                error.retry.retry_after_ms = Some(seconds * 1000);
                budget.schedule(&mut error, false, attempts, 5, false, false);
                std::future::ready(if attempts == 1 { Err(error) } else { Ok(()) })
            },
            |attempt, limit, delay| {
                observed.push((attempt, limit, delay));
                std::future::ready(Ok(()))
            },
        )
        .await;
        if seconds <= 60 {
            assert!(result.is_ok());
            assert_eq!(attempts, 2);
            assert_eq!(observed, [(1, 5, std::time::Duration::from_secs(seconds))]);
        } else {
            let error = result.unwrap_err();
            assert_eq!(attempts, 1);
            assert!(observed.is_empty());
            assert!(
                super::super::error(error)
                    .message
                    .contains(&(seconds * 1000).to_string())
            );
        }
    }
}
