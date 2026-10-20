use super::*;
use sailry_protocol::{Authentication, CredentialId, ProviderId};
use std::sync::atomic::{AtomicUsize, Ordering};

fn credential(revision: u64) -> Credential {
    Credential {
        id: CredentialId::new(),
        provider: ProviderId::new(),
        authentication: Authentication::ChatGptOAuth,
        account_label: None,
        revision,
        expires_at_ms: None,
        revoked: false,
    }
}

fn response(status: u16) -> http::Response {
    http::Response {
        status,
        headers: BTreeMap::new(),
        body: "fixture".into(),
    }
}

#[tokio::test]
async fn shares_overlap_and_recovers_failures() {
    let cache = Cache::default();
    let policy = Policy::new([1; 32], 60000);
    let key = policy.key(&credential(1));
    let stop = CancellationToken::new();
    let calls = AtomicUsize::new(0);
    let (first, second) = tokio::join!(
        cache.fetch(key, policy, &stop, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(response(503))
        }),
        cache.fetch(key, policy, &stop, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(response(200))
        })
    );
    assert_eq!(first.unwrap().status, 503);
    assert_eq!(second.unwrap().status, 503);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let later = Policy::new([1; 32], 60000);
    assert_eq!(
        cache
            .fetch(key, later, &stop, || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(response(200))
            })
            .await
            .unwrap()
            .status,
        200
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        cache
            .fetch(key, Policy::new([1; 32], 60000), &stop, || async {
                panic!("a fresh success must be shared")
            })
            .await
            .unwrap()
            .status,
        200
    );
}

#[tokio::test]
async fn expires_success_without_stale_fallback() {
    let cache = Cache::default();
    let key = [2; 32];
    let slot = cache.slot(key).unwrap();
    *slot.lock().await = Some(Document {
        result: Ok(response(200)),
        fetched: Instant::now() - Duration::from_secs(120),
    });
    drop(slot);
    let stop = CancellationToken::new();
    let policy = Policy::new([2; 32], 60000);
    let error = cache
        .fetch(key, policy, &stop, || async {
            Err(Fault::new(ErrorCode::Unavailable, "fixture failure"))
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    let shared = cache
        .fetch(key, policy, &stop, || async {
            panic!("overlapping readers must share the failure")
        })
        .await
        .unwrap_err();
    assert_eq!(shared, error);
}

#[test]
fn binds_credential_revisions_and_active_slots() {
    let policy = Policy::new([3; 32], 60000);
    let mut first = credential(1);
    let before = policy.key(&first);
    first.revision += 1;
    let refreshed = policy.key(&first);
    assert_ne!(before, refreshed);
    first.id = CredentialId::new();
    assert_ne!(refreshed, policy.key(&first));
    let cache = Cache::default();
    let mut held = Vec::new();
    for value in 0..CAPACITY {
        held.push(cache.slot([value as u8; 32]).unwrap());
    }
    assert_eq!(
        cache
            .slot([255; 32])
            .err()
            .expect("active slots cannot be evicted")
            .code,
        ErrorCode::Busy
    );
    held.pop();
    assert!(cache.slot([255; 32]).is_ok());
}
