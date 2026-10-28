use super::*;

#[tokio::test]
async fn reuses_committed_outcome() {
    let mut fixture = fixture::Fixture::new().await;
    let turn = fixture.completed(&["Canonical answer"]).await;
    let before = fixture.read(None, 100).await;
    let mut updates = fixture.events.subscribe();
    // An uncertain caller may repeat settlement after its commit succeeded.
    let status = runs::finish(
        &mut fixture.database,
        turn,
        Status::Cancelled,
        Some(Fault::new(ErrorCode::Internal, "lost acknowledgement")),
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(status, Status::Completed);
    assert_eq!(fixture.read(None, 100).await, before);
    assert!(updates.try_recv().is_err());
}
