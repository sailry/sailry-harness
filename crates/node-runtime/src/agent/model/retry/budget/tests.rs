use super::*;

fn failure(class: Class, retry_at: Option<Instant>) -> Failure {
    Failure {
        class,
        retry_at,
        transport: None,
        status: None,
        code: None,
        refused: false,
        switch: false,
    }
}

#[tokio::test(start_paused = true)]
async fn shares_five_retries_across_failures() {
    let mut budget = Budget::default();
    for (index, class) in [
        Class::Connection,
        Class::Backoff,
        Class::Advice(Stop::RateLimited),
        Class::Advice(Stop::ServerOverloaded),
        Class::Connection,
    ]
    .into_iter()
    .enumerate()
    {
        let now = Instant::now();
        let Decision::Retry {
            attempt,
            limit,
            at,
            delay,
        } = budget.next(&failure(class, None), 5)
        else {
            panic!("retry expected")
        };
        assert_eq!((attempt, limit), (index as u64 + 1, Some(5)));
        assert_eq!(delay, Some(Duration::from_secs(60)));
        assert_eq!(at.duration_since(now), Duration::from_secs(60));
        tokio::time::sleep_until(at).await;
    }
    assert!(matches!(
        budget.next(&failure(Class::Connection, None), 5),
        Decision::Exhausted
    ));
    assert!(matches!(
        Budget::default().next(&failure(Class::Backoff, None), 0),
        Decision::Exhausted
    ));
}

#[tokio::test(start_paused = true)]
async fn applies_minimum_and_longer_server_delay() {
    for seconds in [0, 2, 60, 90, 86_400] {
        let now = Instant::now();
        let error = failure(
            Class::Advice(Stop::RateLimited),
            Some(now + Duration::from_secs(seconds)),
        );
        let Decision::Retry { at, .. } = Budget::default().next(&error, 5) else {
            panic!("server advice expected")
        };
        assert_eq!(at.duration_since(now), Duration::from_secs(seconds.max(60)));
    }
}

#[tokio::test(start_paused = true)]
async fn keeps_the_server_deadline() {
    let at = Instant::now() + Duration::from_secs(90);
    let error = failure(Class::Advice(Stop::RateLimited), Some(at));
    tokio::time::advance(Duration::from_secs(7)).await;
    let Decision::Retry {
        at: planned, delay, ..
    } = Budget::default().next(&error, 5)
    else {
        panic!("server advice expected")
    };
    assert_eq!(planned, at);
    assert_eq!(
        planned.duration_since(Instant::now()),
        Duration::from_secs(83)
    );
    assert!(delay.is_none());
}

#[test]
fn stops_non_retryable_failures() {
    assert!(matches!(
        Budget::default().next(&failure(Class::Terminal(Stop::InvalidRequest), None), 5),
        Decision::Stop(Stop::InvalidRequest)
    ));
}
