use super::*;
use iroh::{RelayMode, endpoint::presets};

#[tokio::test]
async fn isolates_pending_inspection_and_revocation() {
    let directory = tempfile::tempdir().unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let host = Link::controller(directory.path().join("host"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = host.handle().invite().unwrap();
    controller.handle().pair(invitation.ticket()).await.unwrap();
    let blocked = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .unwrap();
    let address = blocked.addr();
    let peer = NodeId(*address.id.as_bytes());
    let handle = controller.handle();
    handle.set_trust(peer, true).await.unwrap();
    let dialer = handle.clone();
    let dial = tokio::spawn(async move { dialer.inspect(&address).await });
    let connection = tokio::time::timeout(Duration::from_secs(3), async {
        blocked.accept().await.unwrap().await.unwrap()
    })
    .await
    .unwrap();
    let (mut send, mut recv) = connection.accept_bi().await.unwrap();
    let request: frame::Envelope =
        tokio::time::timeout(Duration::from_secs(3), frame::read(&mut recv))
            .await
            .unwrap()
            .unwrap();
    assert!(matches!(request.call, frame::Call::Inspect));
    // The actual Inspect request has reached the peer, which deliberately holds
    // its reply. Another peer and trust control must not wait for its deadline.
    assert!(!dial.is_finished());
    tokio::time::timeout(
        Duration::from_secs(2),
        handle.inspect(&host.handle().address()),
    )
    .await
    .unwrap()
    .unwrap();
    let waiting_handle = handle.clone();
    let waiting_address = blocked.addr();
    let waiting = tokio::spawn(async move { waiting_handle.inspect(&waiting_address).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let counted = handle
                .core
                .outgoing
                .lock()
                .await
                .get(&peer)
                .is_some_and(|slot| Arc::strong_count(slot) >= 3);
            if counted {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), handle.set_trust(peer, false))
        .await
        .unwrap()
        .unwrap();
    let _ = frame::write(
        &mut send,
        &frame::Reply::Endpoint {
            software_version: Some(crate::PRODUCT_VERSION.into()),
            execution: false,
            name: None,
            platform: "fixture".into(),
        },
    )
    .await;
    let _ = send.finish();
    let error = tokio::time::timeout(Duration::from_secs(2), dial)
        .await
        .unwrap()
        .unwrap()
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap()
            .err()
            .unwrap()
            .code,
        ErrorCode::Conflict
    );

    // The cancelled slot remains counted until its producer/waiters have exited.
    let next = handle.core.slot(NodeId([200; 32])).await.unwrap();
    assert!(!handle.core.outgoing.lock().await.contains_key(&peer));
    drop(next);
    controller.close().await.unwrap();
    host.close().await.unwrap();
    blocked.close().await;
}

#[tokio::test]
async fn counts_dials_and_waiters() {
    let directory = tempfile::tempdir().unwrap();
    let link = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let core = &link.handle.core;
    let mut leases = Vec::new();
    for index in 0..MAX_CONNECTIONS {
        leases.push(core.slot(NodeId([index as u8; 32])).await.unwrap());
    }
    let waiting = leases.pop().unwrap();
    assert_eq!(
        core.slot(NodeId([200; 32])).await.err().unwrap().code,
        ErrorCode::Busy
    );
    // A waiter keeps the failed/empty slot counted until its lease is released.
    let second = waiting.clone();
    drop(waiting);
    assert_eq!(
        core.slot(NodeId([200; 32])).await.err().unwrap().code,
        ErrorCode::Busy
    );
    drop(second);
    let next = core.slot(NodeId([200; 32])).await.unwrap();
    assert_eq!(core.outgoing.lock().await.len(), MAX_CONNECTIONS);
    drop(next);
    drop(leases);
    link.close().await.unwrap();
}
