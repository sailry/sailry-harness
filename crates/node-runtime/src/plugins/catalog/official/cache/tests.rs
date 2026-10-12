use super::*;
use std::sync::Arc;

fn entries() -> Vec<Entry> {
    bundled::entries().unwrap()
}

async fn seed(cache: &Cache) -> Vec<Entry> {
    let entries = entries();
    assert_eq!(
        cache
            .fetch(INDEX, false, &CancellationToken::new(), || async {
                Ok(entries.clone())
            })
            .await
            .unwrap(),
        entries
    );
    entries
}

#[tokio::test]
async fn reuses_fresh_entries_without_extending_ttl() {
    let cache = Cache::default();
    let expected = seed(&cache).await;
    let fetched = cache.0.lock().await.as_ref().unwrap().fetched;
    let actual = cache
        .fetch(INDEX, false, &CancellationToken::new(), || async {
            panic!("fresh entries must not be fetched again")
        })
        .await
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(cache.0.lock().await.as_ref().unwrap().fetched, fetched);
}

#[tokio::test]
async fn replaces_expired_entries() {
    let cache = Cache::default();
    let mut expected = seed(&cache).await;
    cache.0.lock().await.as_mut().unwrap().fetched = Instant::now() - TTL;
    expected[0].version = Some("fresh".into());
    assert_eq!(
        cache
            .fetch(INDEX, false, &CancellationToken::new(), || async {
                Ok(expected.clone())
            })
            .await
            .unwrap(),
        expected
    );
    assert_eq!(cache.0.lock().await.as_ref().unwrap().entries, expected);
    assert!(cache.0.lock().await.as_ref().unwrap().fetched.elapsed() < TTL);
}

#[tokio::test]
async fn force_replaces_fresh_entries() {
    let cache = Cache::default();
    let mut expected = seed(&cache).await;
    expected[0].version = Some("forced".into());
    assert_eq!(
        cache
            .fetch(INDEX, true, &CancellationToken::new(), || async {
                Ok(expected.clone())
            })
            .await
            .unwrap(),
        expected
    );
    assert_eq!(
        cache
            .fetch(INDEX, false, &CancellationToken::new(), || async {
                panic!("forced entries must be shared with ordinary requests")
            })
            .await
            .unwrap(),
        expected
    );
}

#[tokio::test]
async fn failure_preserves_entries_and_timestamp() {
    let cache = Cache::default();
    let expected = seed(&cache).await;
    let fetched = cache.0.lock().await.as_ref().unwrap().fetched;
    for code in [ErrorCode::Unavailable, ErrorCode::InvalidRequest] {
        let error = Fault::new(code, "injected catalog failure");
        assert_eq!(
            cache
                .fetch(INDEX, true, &CancellationToken::new(), || async {
                    Err(error.clone())
                })
                .await
                .unwrap_err(),
            error
        );
        let document = cache.0.lock().await;
        let document = document.as_ref().unwrap();
        assert_eq!(document.entries, expected);
        assert_eq!(document.fetched, fetched);
    }
}

#[tokio::test]
async fn failed_miss_does_not_cache() {
    let cache = Cache::default();
    for _ in 0..2 {
        assert_eq!(
            cache
                .fetch(INDEX, false, &CancellationToken::new(), || async {
                    Err(Fault::new(
                        ErrorCode::Unavailable,
                        "injected catalog failure",
                    ))
                })
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        assert!(cache.0.lock().await.is_none());
    }
    seed(&cache).await;
}

#[tokio::test]
async fn source_identity_does_not_share_entries() {
    let cache = Cache::default();
    let mut expected = seed(&cache).await;
    expected[0].version = Some("other source".into());
    assert_eq!(
        cache
            .fetch(
                "http://127.0.0.1/catalog.json",
                false,
                &CancellationToken::new(),
                || async { Ok(expected.clone()) }
            )
            .await
            .unwrap(),
        expected
    );
    assert_eq!(
        cache.0.lock().await.as_ref().unwrap().source,
        "http://127.0.0.1/catalog.json"
    );
}

#[tokio::test]
async fn concurrent_calls_share_one_fetch() {
    let cache = Arc::new(Cache::default());
    let first = cache.clone();
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        first
            .fetch(INDEX, false, &CancellationToken::new(), || async {
                entered.send(()).unwrap();
                released.await.unwrap();
                Ok(entries())
            })
            .await
            .unwrap()
    });
    waiting.await.unwrap();
    let stop = CancellationToken::new();
    let mut second = Box::pin(cache.fetch(INDEX, false, &stop, || async {
        panic!("simultaneous requests must share the completed fetch")
    }));
    assert!(futures::poll!(&mut second).is_pending());
    release.send(()).unwrap();
    assert_eq!(second.await.unwrap(), task.await.unwrap());
}

#[tokio::test]
async fn cancellation_does_not_return_cached_entries() {
    let cache = Cache::default();
    seed(&cache).await;
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        cache
            .fetch(INDEX, false, &stop, || async {
                panic!("cancelled requests must not fetch")
            })
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}

#[tokio::test]
async fn cancellation_interrupts_lock_wait() {
    let cache = Cache::default();
    let _guard = cache.0.lock().await;
    let stop = CancellationToken::new();
    let mut fetch = Box::pin(cache.fetch(INDEX, false, &stop, || async {
        panic!("cancelled waiter must not fetch")
    }));
    assert!(futures::poll!(&mut fetch).is_pending());
    stop.cancel();
    assert_eq!(fetch.await.unwrap_err().code, ErrorCode::Cancelled);
}

#[tokio::test]
async fn cancellation_during_fetch_preserves_cache() {
    let cache = Cache::default();
    let expected = seed(&cache).await;
    let fetched = cache.0.lock().await.as_ref().unwrap().fetched;
    let stop = CancellationToken::new();
    let mut fetch = Box::pin(cache.fetch(INDEX, true, &stop, || async {
        std::future::pending::<Result<Vec<Entry>, Fault>>().await
    }));
    assert!(futures::poll!(&mut fetch).is_pending());
    stop.cancel();
    assert_eq!(fetch.await.unwrap_err().code, ErrorCode::Cancelled);
    let document = cache.0.lock().await;
    let document = document.as_ref().unwrap();
    assert_eq!(document.entries, expected);
    assert_eq!(document.fetched, fetched);
}

#[tokio::test]
async fn cancellation_before_publish_keeps_slot_empty() {
    let cache = Cache::default();
    let stop = CancellationToken::new();
    assert_eq!(
        cache
            .fetch(INDEX, false, &stop, || async {
                stop.cancel();
                Ok(entries())
            })
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert!(cache.0.lock().await.is_none());
}
