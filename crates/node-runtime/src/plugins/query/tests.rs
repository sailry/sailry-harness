use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn shares_keys_without_serializing_other_reads() {
    let cache = Arc::new(Cache::<u8, u8>::default());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let arrived = Arc::new(tokio::sync::Semaphore::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let first = tokio::spawn({
        let (cache, gate, arrived, calls) =
            (cache.clone(), gate.clone(), arrived.clone(), calls.clone());
        async move {
            cache
                .fetch(
                    1,
                    Duration::from_secs(60),
                    false,
                    &CancellationToken::new(),
                    || async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        arrived.add_permits(1);
                        gate.acquire().await.unwrap().forget();
                        Ok(7)
                    },
                )
                .await
        }
    });
    arrived.acquire().await.unwrap().forget();
    let other = tokio::time::timeout(
        Duration::from_secs(1),
        cache.fetch(
            2,
            Duration::from_secs(60),
            false,
            &CancellationToken::new(),
            || async { Ok(8) },
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(other, 8);
    let stop = CancellationToken::new();
    let mut second = Box::pin(
        cache.fetch(1, Duration::from_secs(60), true, &stop, || async {
            panic!("identical read must share acquisition")
        }),
    );
    assert!(futures::poll!(&mut second).is_pending());
    gate.add_permits(1);
    assert_eq!(first.await.unwrap().unwrap(), 7);
    assert_eq!(second.await.unwrap(), 7);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn preserves_success_and_retries_failed_refreshes() {
    let cache = Cache::<u8, u8>::default();
    let stop = CancellationToken::new();
    cache.insert(1, 7).unwrap();
    let failure = cache
        .fetch(1, Duration::from_secs(60), true, &stop, || async {
            Err(Fault::new(ErrorCode::Unavailable, "fixture unavailable"))
        })
        .await
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::Unavailable);
    assert_eq!(cache.peek(&1), Some(7));
    assert_eq!(
        cache
            .fetch(1, Duration::from_secs(60), false, &stop, || async { Ok(9) })
            .await
            .unwrap(),
        9
    );
    assert_eq!(cache.peek(&1), Some(9));
    stop.cancel();
    assert_eq!(
        cache
            .fetch(1, Duration::ZERO, true, &stop, || async {
                panic!("cancelled read must not start")
            })
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}

#[tokio::test]
async fn evicts_only_idle_keys() {
    let cache = Cache::<usize, usize>::default();
    for key in 0..=CAPACITY {
        cache.insert(key, key).unwrap();
    }
    assert_eq!(cache.peek(&0), None);
    assert_eq!(cache.peek(&CAPACITY), Some(CAPACITY));
}

#[tokio::test]
async fn shares_failures_only_during_acquisition() {
    let cache = Arc::new(Cache::<u8, u8>::default());
    cache.insert(1, 3).unwrap();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let arrived = Arc::new(tokio::sync::Semaphore::new(0));
    let first = tokio::spawn({
        let (cache, gate, arrived) = (cache.clone(), gate.clone(), arrived.clone());
        async move {
            cache
                .fetch(
                    1,
                    Duration::from_secs(60),
                    true,
                    &CancellationToken::new(),
                    || async {
                        arrived.add_permits(1);
                        gate.acquire().await.unwrap().forget();
                        Err(Fault::new(ErrorCode::Unavailable, "fixture unavailable"))
                    },
                )
                .await
        }
    });
    arrived.acquire().await.unwrap().forget();
    let stop = CancellationToken::new();
    let mut second = Box::pin(
        cache.fetch(1, Duration::from_secs(60), true, &stop, || async {
            panic!("overlapping failure must share acquisition")
        }),
    );
    assert!(futures::poll!(&mut second).is_pending());
    gate.add_permits(1);
    assert_eq!(
        first.await.unwrap().unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(second.await.unwrap_err().code, ErrorCode::Unavailable);
    assert_eq!(cache.peek(&1), Some(3));
    assert_eq!(
        cache
            .fetch(1, Duration::from_secs(60), false, &stop, || async { Ok(4) })
            .await
            .unwrap(),
        4
    );
}

#[tokio::test]
async fn admits_active_keys_beyond_cache_retention() {
    let cache = Arc::new(Cache::<usize, usize>::default());
    let arrived = Arc::new(tokio::sync::Semaphore::new(0));
    let release = CancellationToken::new();
    let count = CAPACITY + 2;
    let pending: Vec<_> = (0..count)
        .map(|key| {
            let (cache, arrived, release) = (cache.clone(), arrived.clone(), release.clone());
            tokio::spawn(async move {
                cache
                    .fetch(
                        key,
                        Duration::from_secs(60),
                        false,
                        &CancellationToken::new(),
                        || async {
                            arrived.add_permits(1);
                            release.cancelled().await;
                            Ok(key)
                        },
                    )
                    .await
            })
        })
        .collect();
    let entered =
        tokio::time::timeout(Duration::from_secs(2), arrived.acquire_many(count as u32)).await;
    assert!(
        entered.is_ok(),
        "independent cache acquisitions must start together"
    );
    entered.unwrap().unwrap().forget();
    let stop = CancellationToken::new();
    let mut shared =
        std::pin::pin!(
            cache.fetch(0, Duration::from_secs(60), true, &stop, || async {
                panic!("an active key must retain its acquisition")
            })
        );
    assert!(futures::poll!(&mut shared).is_pending());
    release.cancel();
    assert_eq!(shared.await.unwrap(), 0);
    for (key, result) in futures::future::join_all(pending)
        .await
        .into_iter()
        .enumerate()
    {
        assert_eq!(result.unwrap().unwrap(), key);
    }
    assert!(cache.0.lock().unwrap().len() <= CAPACITY);
}
