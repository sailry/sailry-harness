use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, ErrorCode, Output};

#[tokio::test]
async fn restores_uncached_trust() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("host");
    let host = Node::start(&path).await.unwrap();
    let client = Node::start(directory.path().join("client")).await.unwrap();
    host.link().set_trust(client.id(), true).await.unwrap();
    client.link().set_trust(host.id(), true).await.unwrap();
    host.shutdown().await.unwrap();
    let host = Node::start(&path).await.unwrap();
    assert!(host.link().peers().await.unwrap().is_empty());
    let remote = Client::new(client.link().remote(host.link().address()));
    assert!(
        remote
            .execute(remote.prepare(Command::Snapshot))
            .await
            .is_ok()
    );
    let invitation = host.link().invite().unwrap();
    client.link().pair(invitation.ticket()).await.unwrap();
    assert_eq!(
        host.link().peers().await.unwrap(),
        vec![client.link().address()]
    );
    host.shutdown().await.unwrap();
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn persists_certified_trust() {
    let directory = tempfile::tempdir().unwrap();
    let host_path = directory.path().join("host");
    let client_path = directory.path().join("client");
    let host = Node::start(&host_path).await.unwrap();
    let controller = Node::start(&client_path).await.unwrap();
    let controller_address = controller.link().address();
    let host_address = host.link().address();
    let mut invitation = host.link().invite().unwrap();
    let address = controller.link().pair(invitation.ticket()).await.unwrap();
    invitation.paired().await.unwrap();
    assert_eq!(
        host.link().peers().await.unwrap(),
        vec![controller_address.clone()]
    );
    assert_eq!(
        controller.link().peers().await.unwrap(),
        vec![host_address.clone()]
    );
    let client = Client::new(controller.link().remote(address));
    assert!(matches!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap(),
        Output::Snapshot(_)
    ));
    drop(invitation);
    host.shutdown().await.unwrap();
    controller.shutdown().await.unwrap();
    let host = Node::start(host_path).await.unwrap();
    let controller = Node::start(client_path).await.unwrap();
    assert_eq!(host.link().peers().await.unwrap(), vec![controller_address]);
    assert_eq!(controller.link().peers().await.unwrap(), vec![host_address]);
    let client = Client::new(controller.link().remote(host.link().address()));
    assert!(matches!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap(),
        Output::Snapshot(_)
    ));
    host.shutdown().await.unwrap();
    controller.shutdown().await.unwrap();
}

#[tokio::test]
async fn certifies_one_claimant() {
    let directory = tempfile::tempdir().unwrap();
    let host = Node::start(directory.path().join("host")).await.unwrap();
    let first = Node::start(directory.path().join("first")).await.unwrap();
    let second = Node::start(directory.path().join("second")).await.unwrap();
    let invitation = host.link().invite().unwrap();
    let first_link = first.link();
    let second_link = second.link();
    let (one, two) = tokio::join!(
        first_link.pair(invitation.ticket()),
        second_link.pair(invitation.ticket())
    );
    assert_ne!(one.is_ok(), two.is_ok());
    let winner = if one.is_ok() { &first } else { &second };
    assert!(winner.link().pair(invitation.ticket()).await.is_ok());
    host.link().set_trust(winner.id(), false).await.unwrap();
    assert!(host.link().peers().await.unwrap().is_empty());
    assert!(winner.link().pair(invitation.ticket()).await.is_err());
    host.shutdown().await.unwrap();
    first.shutdown().await.unwrap();
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn revokes_replaced_or_dropped_tickets() {
    let directory = tempfile::tempdir().unwrap();
    let host = Node::start(directory.path().join("host")).await.unwrap();
    let controller = Node::start(directory.path().join("client")).await.unwrap();
    let mut previous = host.link().invite().unwrap();
    let current = host.link().invite().unwrap();
    assert!(previous.paired().await.is_err());
    assert!(controller.link().pair(previous.ticket()).await.is_err());
    let cancelled = current.ticket().to_owned();
    drop(current);
    assert!(controller.link().pair(&cancelled).await.is_err());
    let client = Client::new(controller.link().remote(host.link().address()));
    assert_eq!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    host.shutdown().await.unwrap();
    controller.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_invalid_tickets() {
    let directory = tempfile::tempdir().unwrap();
    let host = Node::start(directory.path().join("host")).await.unwrap();
    let controller = Node::start(directory.path().join("client")).await.unwrap();
    let invitation = host.link().invite().unwrap();
    let mut forged: serde_json::Value = serde_json::from_str(invitation.ticket()).unwrap();
    forged["token"] = serde_json::json!(vec![0; 32]);
    assert!(controller.link().pair(&forged.to_string()).await.is_err());
    let mut expired: serde_json::Value = serde_json::from_str(invitation.ticket()).unwrap();
    expired["expires_at_ms"] = serde_json::json!(0);
    assert!(controller.link().pair(&expired.to_string()).await.is_err());
    assert!(controller.link().pair(invitation.ticket()).await.is_ok());
    host.shutdown().await.unwrap();
    controller.shutdown().await.unwrap();
}
