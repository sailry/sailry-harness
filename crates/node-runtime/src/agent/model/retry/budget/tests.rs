use super::*;

fn failure(class: Class, retry_at: Option<Instant>) -> Failure {
    Failure {
        class,
        retry_at,
        transport: None,
        status: None,
        code: None,
    }
}

#[tokio::test(start_paused = true)]
async fn separates_connection_and_finite_counters() {
    let mut budget = Budget::default();
    for (index, seconds) in [5, 10, 20, 40, 60, 60].into_iter().enumerate() {
        let now = Instant::now();
        let Decision::Retry {
            attempt, limit, at, ..
        } = budget.next(&failure(Class::Connection, None), 5, true)
        else {
            panic!("persistent recovery expected")
        };
        assert_eq!((attempt, limit), (index as u64 + 1, None));
        assert_eq!(at.duration_since(now), Duration::from_secs(seconds));
        tokio::time::sleep_until(at).await;
    }
    for count in 1..=5 {
        assert!(
            matches!(budget.next(&failure(Class::Backoff, None), 5, true), Decision::Retry { attempt, limit: Some(5), .. } if attempt == count)
        );
    }
    assert!(matches!(
        budget.next(&failure(Class::Backoff, None), 5, true),
        Decision::Exhausted
    ));
    budget.fallback();
    assert!(matches!(
        budget.next(&failure(Class::Backoff, None), 5, true),
        Decision::Retry { attempt: 1, .. }
    ));
    assert!(matches!(
        Budget::default().next(&failure(Class::Connection, None), 0, false),
        Decision::Exhausted
    ));
}

#[tokio::test(start_paused = true)]
async fn preserves_advice_deadlines_without_caps() {
    let at = Instant::now() + Duration::from_secs(86_400);
    let error = failure(Class::Advice(Stop::RateLimited), Some(at));
    tokio::time::advance(Duration::from_secs(7)).await;
    let Decision::Retry { at: planned, .. } = Budget::default().next(&error, 5, false) else {
        panic!("server advice expected")
    };
    assert_eq!(planned, at);
    assert_eq!(
        planned.duration_since(Instant::now()),
        Duration::from_secs(86_393)
    );
    assert!(matches!(
        Budget::default().next(&failure(Class::Advice(Stop::RateLimited), None), 5, false),
        Decision::Stop(Stop::RateLimited)
    ));
}

#[test]
fn uses_codex_jitter_envelope() {
    for (attempt, millis) in [(1, 200), (2, 400), (3, 800), (4, 1600), (5, 3200)] {
        let delay = backoff(attempt).as_millis();
        assert!(delay >= millis * 9 / 10 && delay < millis * 11 / 10);
    }
}
