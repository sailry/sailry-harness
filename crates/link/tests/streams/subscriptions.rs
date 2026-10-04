use super::*;

pub(super) struct Idle(pub bool);

impl Subscription for Idle {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if self.0 {
                std::future::pending::<()>().await;
            }
            self.0 = true;
            Ok(Update::ResetRequired)
        })
    }
}

#[tokio::test]
async fn accepts_commands_at_capacity() {
    let fixture = Fixture::new().await;
    let remote = fixture.remote();
    let mut subscriptions = Vec::new();
    for _ in 0..64 {
        let mut subscription = remote
            .subscribe(Topic::Conversation(SessionId::new()))
            .await
            .unwrap();
        match tokio::time::timeout(Duration::from_secs(2), subscription.next())
            .await
            .unwrap()
        {
            Ok(Update::ResetRequired) => subscriptions.push(subscription),
            Err(Fault {
                code: ErrorCode::Busy,
                ..
            }) => break,
            _ => panic!("expected initial update or capacity rejection"),
        }
    }
    assert!((12..64).contains(&subscriptions.len()));
    let stream = remote.open(fixture.handler.resource).await.unwrap();
    assert!(matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            remote.dispatch(Request::new(fixture.target, Command::Snapshot))
        )
        .await
        .unwrap(),
        Err(Fault {
            code: ErrorCode::NotFound,
            ..
        })
    ));
    drop(stream);
    drop(subscriptions);
    fixture.close().await;
}

#[tokio::test]
async fn conversations_preserve_capacity() {
    let fixture = Fixture::new().await;
    let remote = fixture.remote();
    let mut subscriptions = Vec::new();
    for _ in 0..12 {
        subscriptions.push(
            remote
                .subscribe(Topic::Conversation(SessionId::new()))
                .await
                .unwrap(),
        );
        notified(&fixture.handler.preparing).await;
    }
    assert!(matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            remote.dispatch(Request::new(fixture.target, Command::Snapshot))
        )
        .await
        .unwrap(),
        Err(Fault {
            code: ErrorCode::NotFound,
            ..
        })
    ));
    drop(subscriptions);
    fixture.close().await;
}

pub(super) struct Preparing<'a>(pub &'a tokio::sync::Semaphore);

impl Drop for Preparing<'_> {
    fn drop(&mut self) {
        self.0.add_permits(1);
    }
}

async fn notified(signal: &tokio::sync::Semaphore) {
    tokio::time::timeout(Duration::from_secs(2), signal.acquire())
        .await
        .expect("subscription lifecycle deadline")
        .unwrap()
        .forget();
}

#[tokio::test]
async fn cancellation_releases_capacity() {
    let fixture = Fixture::new().await;
    let remote = fixture.remote();
    // More than one connection's request capacity; no preparation ever finishes.
    for _ in 0..12 {
        let subscription = remote
            .subscribe(Topic::Files(WorktreeId::new()))
            .await
            .unwrap();
        notified(&fixture.handler.preparing).await;
        drop(subscription);
        notified(&fixture.handler.cancelled).await;
    }
    assert!(matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            remote.dispatch(Request::new(fixture.target, Command::Snapshot))
        )
        .await
        .unwrap(),
        Err(Fault {
            code: ErrorCode::NotFound,
            ..
        })
    ));
    fixture.close().await;
}

#[tokio::test]
async fn shutdown_cancels_preparation() {
    let fixture = Fixture::new().await;
    let subscription = fixture
        .remote()
        .subscribe(Topic::Files(WorktreeId::new()))
        .await
        .unwrap();
    notified(&fixture.handler.preparing).await;
    tokio::time::timeout(Duration::from_secs(2), fixture.execution.close())
        .await
        .unwrap()
        .unwrap();
    notified(&fixture.handler.cancelled).await;
    drop(subscription);
    fixture.controller.close().await.unwrap();
}
