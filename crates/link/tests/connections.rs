use sailry_link::{CancellationToken, ConnectionInfo, Link, NetworkScope, Route};
use sailry_protocol::NodeId;
use std::time::Duration;
use tokio::sync::watch;

async fn until(receiver: &mut watch::Receiver<ConnectionInfo>, route: Option<Route>) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if receiver.borrow_and_update().route == route {
                return;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn follows_disconnect_and_reconnect() {
    let directory = tempfile::tempdir().unwrap();
    let host = Link::controller(directory.path().join("host"), NetworkScope::default())
        .await
        .unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = host.handle().invite().unwrap();
    controller.handle().pair(invitation.ticket()).await.unwrap();
    let handle = controller.handle();
    let target = host.handle().address();
    let remote = handle.remote(target.clone());
    let stop = CancellationToken::new();
    let (sender, mut receiver) = watch::channel(ConnectionInfo::default());
    let cancellation = stop.clone();
    let task = tokio::spawn(async move {
        remote.watch_connection(sender, cancellation).await;
    });
    until(&mut receiver, Some(Route::P2p)).await;
    assert!(receiver.borrow().rtt_ms.is_some());
    handle.disconnect(NodeId(*target.id.as_bytes())).await;
    until(&mut receiver, None).await;
    assert_eq!(receiver.borrow().rtt_ms, None);
    until(&mut receiver, Some(Route::P2p)).await;
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*receiver.borrow(), ConnectionInfo::default());
    let remote = handle.remote(target);
    let (sender, mut receiver) = watch::channel(ConnectionInfo::default());
    let task = tokio::spawn(async move {
        remote
            .watch_connection(sender, CancellationToken::new())
            .await;
    });
    until(&mut receiver, Some(Route::P2p)).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(*receiver.borrow(), ConnectionInfo::default());
    host.close().await.unwrap();
    controller.close().await.unwrap();
}
