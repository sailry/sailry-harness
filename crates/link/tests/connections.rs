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

#[tokio::test]
async fn follows_fresh_peer_addresses() {
    let directory = tempfile::tempdir().unwrap();
    let host_path = directory.path().join("host");
    let mut host = Link::controller(&host_path, NetworkScope::default())
        .await
        .unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let handle = controller.handle();
    let target = handle
        .pair(host.handle().invite().unwrap().ticket())
        .await
        .unwrap();
    let peer = NodeId(*target.id.as_bytes());
    let remote = handle.remote(target.clone());
    for pair in [false, true] {
        // Reserve a distinct port while the previous endpoint is still bound.
        let reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let bind = reservation.local_addr().unwrap();
        assert!(
            !host
                .handle()
                .address()
                .ip_addrs()
                .any(|address| *address == bind)
        );
        host.close().await.unwrap();
        handle.disconnect(peer).await;
        drop(reservation);
        host = Link::controller(&host_path, NetworkScope::Direct(bind))
            .await
            .unwrap();
        let fresh = host.handle().address();
        assert_eq!(fresh.id, target.id);
        assert!(fresh.ip_addrs().any(|address| *address == bind));
        if pair {
            assert_eq!(
                handle
                    .pair(host.handle().invite().unwrap().ticket())
                    .await
                    .unwrap(),
                fresh
            );
        } else {
            handle.update_peer(fresh).await.unwrap();
        }
        handle.disconnect(peer).await;
        let stop = CancellationToken::new();
        let (sender, mut receiver) = watch::channel(ConnectionInfo::default());
        let task = tokio::spawn({
            let remote = remote.clone();
            let stop = stop.clone();
            async move { remote.watch_connection(sender, stop).await }
        });
        until(&mut receiver, Some(Route::P2p)).await;
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*receiver.borrow(), ConnectionInfo::default());
    }
    host.close().await.unwrap();
    controller.close().await.unwrap();
}
