use super::*;
use std::sync::Arc;

#[tokio::test]
async fn starts_independent_queries_together() {
    let discovery = Arc::new(Discovery::new());
    let arrived = Arc::new(tokio::sync::Semaphore::new(0));
    let release = CancellationToken::new();
    let pending: Vec<_> = (0..12)
        .map(|index| {
            let discovery = discovery.clone();
            let arrived = arrived.clone();
            let release = release.clone();
            tokio::spawn(async move {
                discovery
                    .query(
                        async {
                            arrived.add_permits(1);
                            release.cancelled().await;
                            Ok(index)
                        },
                        CancellationToken::new(),
                    )
                    .await
            })
        })
        .collect();
    let entered = tokio::time::timeout(Duration::from_secs(2), arrived.acquire_many(12)).await;
    release.cancel();
    entered
        .expect("independent discovery queries must start together")
        .unwrap()
        .forget();
    for (index, result) in futures::future::join_all(pending)
        .await
        .into_iter()
        .enumerate()
    {
        assert_eq!(result.unwrap().unwrap(), index);
    }
}
