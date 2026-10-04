use super::*;

async fn notified(signal: &tokio::sync::Semaphore) {
    tokio::time::timeout(Duration::from_secs(2), signal.acquire())
        .await
        .expect("command lifecycle deadline")
        .unwrap()
        .forget();
}

#[tokio::test]
async fn cancels_transient_preparation() {
    let fixture = Fixture::new().await;
    for _ in 0..12 {
        let remote = fixture.remote();
        let request = Request::new(fixture.target, Command::InspectHost);
        let job = tokio::spawn(async move { remote.dispatch(request).await });
        notified(&fixture.handler.preparing).await;
        job.abort();
        let _ = job.await;
        notified(&fixture.handler.cancelled).await;
    }
    fixture.close().await;
}

#[tokio::test]
async fn retains_unobserved_preparation() {
    let fixture = Fixture::new().await;
    let remote = fixture.remote();
    let request = Request::new(
        fixture.target,
        Command::RegisterProject {
            name: "fixture".into(),
            path: "fixture".into(),
        },
    );
    let job = tokio::spawn(async move { remote.dispatch(request).await });
    notified(&fixture.handler.preparing).await;
    job.abort();
    let _ = job.await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            fixture.handler.cancelled.acquire()
        )
        .await
        .is_err()
    );
    fixture.handler.release.add_permits(1);
    notified(&fixture.handler.cancelled).await;
    fixture.close().await;
}
