use super::*;

#[test]
fn cancellation_belongs_to_the_authenticated_caller() {
    let queries = Arc::new(Queries::default());
    let caller = NodeId([1; 32]);
    let foreign = NodeId([2; 32]);
    let request = RequestId::new();
    let closed = CancellationToken::new();
    let active = queries.begin(caller, request, &closed).unwrap();
    let other = queries.begin(foreign, request, &closed).unwrap();
    assert!(matches!(
        queries.begin(caller, request, &closed),
        Err(Fault {
            code: ErrorCode::Busy,
            ..
        })
    ));
    queries.cancel(foreign, request);
    assert!(!active.stop.is_cancelled());
    assert!(other.stop.is_cancelled());
    queries.cancel(caller, request);
    assert!(active.stop.is_cancelled());
    drop(active);
    assert!(queries.begin(caller, request, &closed).is_ok());
}

#[test]
fn shutdown_and_drop_release_the_query() {
    let queries = Arc::new(Queries::default());
    let closed = CancellationToken::new();
    let caller = NodeId([1; 32]);
    let request = RequestId::new();
    let active = queries.begin(caller, request, &closed).unwrap();
    closed.cancel();
    assert!(active.stop.is_cancelled());
    drop(active);
    assert!(queries.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancels_before_a_blocked_notification() {
    let queries = Arc::new(Queries::default());
    let caller = NodeId([1; 32]);
    let request = RequestId::new();
    let active = queries
        .begin(caller, request, &CancellationToken::new())
        .unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    sender.send("queued").await.unwrap();
    let error = {
        let entered = tokio::sync::Notify::new();
        let query = active.run(async {
            entered.notify_one();
            sender.send("blocked").await.unwrap();
            Err(Fault::new(
                ErrorCode::Internal,
                "unexpected notification completion",
            ))
        });
        tokio::pin!(query);
        tokio::select! {
            biased;
            value = &mut query => panic!("unexpected completion: {value:?}"),
            _ = entered.notified() => queries.cancel(caller, request),
        }
        tokio::time::timeout(std::time::Duration::from_secs(1), query)
            .await
            .unwrap()
            .unwrap_err()
    };
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(receiver.recv().await, Some("queued"));
    assert!(receiver.try_recv().is_err());
    drop(active);
    assert!(
        queries
            .begin(caller, request, &CancellationToken::new())
            .is_ok()
    );
}
