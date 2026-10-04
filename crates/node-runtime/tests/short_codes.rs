use sailry_client::Client;
use sailry_link::rendezvous::{Relay, RequestId};
use sailry_link::{CancellationToken, rendezvous::ShareState};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output};
use std::time::Duration;

#[tokio::test]
#[ignore = "run services/pairing-relay/test/rust-e2e.mjs to start real workerd"]
async fn stops_sharing_after_pairing() {
    let relay =
        Relay::new(&std::env::var("SAILRY_TEST_RELAY").expect("test relay address")).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let host = Node::start(directory.path().join("host")).await.unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    let (state, mut updates) = tokio::sync::watch::channel(ShareState::Preparing);
    let stop = CancellationToken::new();
    let link = host.link();
    let drive = relay.share(&link, state, stop.clone());
    let verify = async {
        let first = ready(&mut updates).await;
        tokio::time::sleep(Duration::from_secs(61)).await;
        let next = ready(&mut updates).await;
        assert_ne!(first, next);
        assert!(
            relay
                .pair(&controller.link(), &first, &RequestId::default())
                .await
                .is_err()
        );
        relay
            .pair(&controller.link(), &next, &RequestId::default())
            .await
            .unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(75), async {
        tokio::join!(drive, verify)
    })
    .await
    .unwrap();
    result.unwrap();
    assert!(matches!(*updates.borrow(), ShareState::Paired));
    let (state, mut updates) = tokio::sync::watch::channel(ShareState::Preparing);
    let drive = relay.share(&link, state, stop.clone());
    let verify = async {
        let code = ready(&mut updates).await;
        stop.cancel();
        code
    };
    let (result, code) = tokio::join!(drive, verify);
    result.unwrap();
    assert!(matches!(*updates.borrow(), ShareState::Closed));
    assert!(
        relay
            .pair(&controller.link(), &code, &RequestId::default())
            .await
            .is_err()
    );
    host.shutdown().await.unwrap();
    controller.shutdown().await.unwrap();
}

async fn ready(updates: &mut tokio::sync::watch::Receiver<ShareState>) -> String {
    loop {
        if let ShareState::Ready { code, .. } = updates.borrow_and_update().clone() {
            return code;
        }
        updates.changed().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "run services/pairing-relay/test/rust-e2e.mjs to start real workerd"]
async fn exchanges_code_and_authenticates_peer() {
    let relay =
        Relay::new(&std::env::var("SAILRY_TEST_RELAY").expect("test relay address")).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let host = Node::start(directory.path().join("host")).await.unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    let mut invitation = host.link().invite().unwrap();
    let publish_id = RequestId::default();
    let code = relay.publish(&invitation, &publish_id).await.unwrap();
    assert!(code.expires_at_ms <= invitation.expires_at_ms());
    let retry = relay.publish(&invitation, &publish_id).await.unwrap();
    assert_eq!(code.code, retry.code);
    let claim_id = RequestId::default();
    let address = relay
        .pair(&controller.link(), &code.code, &claim_id)
        .await
        .unwrap();
    invitation.paired().await.unwrap();
    assert_eq!(address, host.link().address());
    assert_eq!(
        relay
            .pair(&controller.link(), &code.code, &claim_id)
            .await
            .unwrap(),
        address
    );
    assert!(
        relay
            .pair(&controller.link(), &code.code, &RequestId::default())
            .await
            .is_err()
    );
    let client = Client::new(controller.link().remote(address));
    assert!(matches!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap(),
        Output::Snapshot(_)
    ));
    relay.cancel(&code).await.unwrap();
    assert!(
        relay
            .pair(&controller.link(), &code.code, &claim_id)
            .await
            .is_err()
    );
    // Removing a publication does not revoke an already paired device.
    assert!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .is_ok()
    );
    host.shutdown().await.unwrap();
    controller.shutdown().await.unwrap();
}
