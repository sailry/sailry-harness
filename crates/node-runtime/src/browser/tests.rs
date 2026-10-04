use super::*;
use serde_json::json;

#[tokio::test]
async fn routes_to_requesting_controller() {
    let browsers = Arc::new(Browsers::default());
    let caller = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    let mut calls = browsers.subscribe(caller).unwrap();
    let _other_calls = browsers.subscribe(other).unwrap();
    assert!(browsers.subscribe(caller).is_err());
    let session = SessionId::new();
    let id = RequestId::new();
    let owner = browsers.clone();
    let pending = tokio::spawn(async move { owner.call(caller, session, id, Action::Tabs).await });
    let Update::BrowserCall(call) = calls.next().await.unwrap() else {
        panic!("browser call expected")
    };
    assert_eq!(call.session, session);
    assert_eq!(call.id, id);
    assert_eq!(
        browsers
            .complete(other, id, Ok(json!({})))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    browsers
        .complete(caller, id, Ok(json!({"tabs":[]})))
        .unwrap();
    assert_eq!(pending.await.unwrap().unwrap(), json!({"tabs":[]}));
    assert!(browsers.complete(caller, id, Ok(json!({}))).is_err());
}

#[tokio::test]
async fn avoids_replay_after_disconnect() {
    let browsers = Arc::new(Browsers::default());
    let caller = NodeId([1; 32]);
    let mut calls = browsers.subscribe(caller).unwrap();
    let owner = browsers.clone();
    let pending = tokio::spawn(async move {
        owner
            .call(
                caller,
                SessionId::new(),
                RequestId::new(),
                Action::Back { tab: 0 },
            )
            .await
    });
    calls.next().await.unwrap();
    drop(calls);
    assert!(!browsers.available(caller));
    assert_eq!(
        pending.await.unwrap().unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    let mut calls = browsers.subscribe(caller).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), calls.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn removes_cancelled_completion() {
    let browsers = Arc::new(Browsers::default());
    let caller = NodeId([1; 32]);
    let mut calls = browsers.subscribe(caller).unwrap();
    let owner = browsers.clone();
    let id = RequestId::new();
    let pending =
        tokio::spawn(async move { owner.call(caller, SessionId::new(), id, Action::Tabs).await });
    calls.next().await.unwrap();
    pending.abort();
    let _ = pending.await;
    assert_eq!(
        browsers
            .complete(caller, id, Ok(json!({})))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}
