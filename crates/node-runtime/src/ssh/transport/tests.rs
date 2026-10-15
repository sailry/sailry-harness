use super::*;
#[cfg(target_os = "macos")]
use sailry_link::CancellationToken;
use std::future::ready;
use tokio::time::Instant;

#[cfg(target_os = "macos")]
#[tokio::test(start_paused = true)]
async fn recovers_after_multiple_failures() {
    let started = Instant::now();
    let mut calls = 0;
    let result = attempt(|| {
        calls += 1;
        ready(if calls < 5 {
            Err(io::Error::from_raw_os_error(65))
        } else {
            Ok("connected")
        })
    })
    .await;
    assert_eq!(result.unwrap(), "connected");
    assert_eq!(calls, 5);
    assert_eq!(
        started.elapsed(),
        RETRY_DELAYS.into_iter().sum::<Duration>()
    );
}

#[cfg(target_os = "macos")]
#[tokio::test(start_paused = true)]
async fn preserves_the_last_connection_error() {
    for kind in [io::ErrorKind::ConnectionRefused, io::ErrorKind::TimedOut] {
        let started = Instant::now();
        let mut calls = 0;
        let error = attempt::<(), _, _>(|| {
            calls += 1;
            ready(Err(if calls == 1 {
                io::Error::from_raw_os_error(65)
            } else {
                io::Error::new(kind, "Connection remains unavailable")
            }))
        })
        .await
        .unwrap_err();
        assert_eq!(calls, 2);
        assert_eq!(error.kind(), kind);
        assert_eq!(error.to_string(), "Connection remains unavailable");
        assert_eq!(started.elapsed(), RETRY_DELAYS[0]);
    }
}

#[cfg(target_os = "macos")]
#[tokio::test(start_paused = true)]
async fn exhausts_bounded_recovery() {
    let started = Instant::now();
    let mut calls = 0;
    let error = attempt::<(), _, _>(|| {
        calls += 1;
        ready(Err(io::Error::new(
            io::ErrorKind::HostUnreachable,
            format!("Unreachable attempt {calls}"),
        )))
    })
    .await
    .unwrap_err();
    assert_eq!(calls, 5);
    assert_eq!(error.to_string(), "Unreachable attempt 5");
    assert_eq!(
        started.elapsed(),
        RETRY_DELAYS.into_iter().sum::<Duration>()
    );
}

#[tokio::test(start_paused = true)]
async fn returns_other_failures_without_retry() {
    for kind in [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::ConnectionRefused,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::NetworkUnreachable,
        io::ErrorKind::TimedOut,
        io::ErrorKind::Other,
    ] {
        let started = Instant::now();
        let mut calls = 0;
        let error = attempt::<(), _, _>(|| {
            calls += 1;
            ready(Err(io::Error::new(kind, "Original connection error")))
        })
        .await
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error.kind(), kind);
        assert_eq!(error.to_string(), "Original connection error");
        assert_eq!(started.elapsed(), Duration::ZERO);
    }
}

#[cfg(not(target_os = "macos"))]
#[tokio::test(start_paused = true)]
async fn leaves_other_platforms_unchanged() {
    let mut calls = 0;
    let error = attempt::<(), _, _>(|| {
        calls += 1;
        ready(Err(io::Error::new(
            io::ErrorKind::HostUnreachable,
            "Unreachable",
        )))
    })
    .await
    .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.kind(), io::ErrorKind::HostUnreachable);
}

#[cfg(target_os = "macos")]
#[tokio::test(start_paused = true)]
async fn retains_the_original_connection_deadline() {
    let started = Instant::now();
    let mut calls = 0;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        attempt::<(), _, _>(|| {
            calls += 1;
            async {
                tokio::time::sleep(Duration::from_secs(20)).await;
                Err(io::Error::from_raw_os_error(65))
            }
        }),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(calls, 2);
    assert_eq!(started.elapsed(), Duration::from_secs(30));
}

#[cfg(target_os = "macos")]
#[tokio::test(start_paused = true)]
async fn deadline_drops_the_pending_retry() {
    let mut calls = 0;
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        attempt::<(), _, _>(|| {
            calls += 1;
            ready(Err(io::Error::from_raw_os_error(65)))
        }),
    )
    .await;
    assert!(result.is_err());
    tokio::time::sleep(RETRY_DELAYS[0]).await;
    assert_eq!(calls, 1);
}

#[cfg(target_os = "macos")]
#[tokio::test(start_paused = true)]
async fn cancellation_drops_the_pending_retry() {
    for close in [false, true] {
        let stop = CancellationToken::new();
        let closed = CancellationToken::new();
        let cancelled = if close { closed.clone() } else { stop.clone() };
        let mut calls = 0;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cancelled.cancel();
        });
        tokio::select! {
            biased;
            _ = stop.cancelled() => {},
            _ = closed.cancelled() => {},
            _ = attempt::<(), _, _>(|| {
                calls += 1;
                ready(Err(io::Error::from_raw_os_error(65)))
            }) => panic!("cancellation must drop the pending TCP retry"),
        }
        tokio::time::sleep(RETRY_DELAYS[0]).await;
        assert_eq!(calls, 1);
    }
}
