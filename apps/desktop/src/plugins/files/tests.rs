use super::*;
use crate::plugins::fixture::Fixture;
use std::time::Duration;

#[test]
fn broadcasts_changes_to_independent_cursors() {
    let fixture = Fixture::new(false);
    let files = Files::new(true);
    let (sender, receiver) = tokio::sync::watch::channel(FileView::default());
    assert!(files.receiver.set(receiver).is_ok());
    let stop = CancellationToken::new();
    let seen = serde_json::json!({"revision": 0, "connected": false}).to_string();
    let next = || {
        files
            .next(
                seen.clone(),
                fixture.binding.client.clone(),
                fixture.session.worktree,
                fixture.runtime.handle().clone(),
                stop.clone(),
            )
            .unwrap()
    };
    let first = next();
    let second = next();
    fixture.runtime.block_on(async {
        let mut first = std::pin::pin!(first);
        let mut second = std::pin::pin!(second);
        assert!(futures::poll!(&mut first).is_pending());
        assert!(futures::poll!(&mut second).is_pending());
        sender.send_modify(|view| {
            view.revision = 7;
            view.connected = true;
        });
        let (first, second) = tokio::time::timeout(
            Duration::from_secs(10),
            futures::future::join(first, second),
        )
        .await
        .expect("concurrent file subscription deadline");
        let expected = serde_json::json!({"revision": 7, "connected": true}).to_string();
        assert_eq!(first.unwrap().as_str(), Some(expected.as_str()));
        assert_eq!(second.unwrap().as_str(), Some(expected.as_str()));
    });
    stop.cancel();
    fixture.close();
}
