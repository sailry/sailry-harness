use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, ErrorCode, NodeId, Output};

#[tokio::test]
async fn dropping_owner_releases_profile() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("controller");
    let link = Link::controller(&profile, NetworkScope::default())
        .await
        .unwrap();
    let handle = link.handle();
    drop(link);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Ok(reopened) = Link::controller(&profile, NetworkScope::default()).await {
            reopened.close().await.unwrap();
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "controller cleanup deadline"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(handle.set_trust(NodeId([0; 32]), true).await.is_err());
}

#[tokio::test]
async fn pairs_without_hosting_node() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("controller");
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let link = Link::controller(&path, NetworkScope::default())
        .await
        .unwrap();
    assert!(
        Link::controller(&path, NetworkScope::default())
            .await
            .is_err()
    );
    let address = link.handle().address();
    let id = NodeId(*address.id.as_bytes());
    let invitation = node.link().invite().unwrap();
    let target = link.handle().pair(invitation.ticket()).await.unwrap();
    assert_eq!(link.handle().peers().await.unwrap(), vec![target.clone()]);
    let client = Client::new(link.handle().remote(target.clone()));
    let request = client.prepare(Command::RegisterProject {
        name: "Controller project".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let output = client.execute(request.clone()).await.unwrap();
    assert!(matches!(output, Output::Project(_)));
    // A paired Node cannot execute business commands on a controller.
    let reverse = Client::new(node.link().remote(address));
    assert_eq!(
        reverse
            .execute(reverse.prepare(Command::Snapshot))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    link.close().await.unwrap();
    assert!(!path.join("storage").exists());
    assert!(Node::start(&path).await.is_err());
    let link = Link::controller(&path, NetworkScope::default())
        .await
        .unwrap();
    // Old Client handles cannot keep the closed endpoint's profile hostage.
    assert!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .is_err()
    );
    drop(client);
    assert_eq!(NodeId(*link.handle().address().id.as_bytes()), id);
    assert_eq!(link.handle().peers().await.unwrap(), vec![target.clone()]);
    let client = Client::new(link.handle().remote(target));
    assert_eq!(client.execute(request).await.unwrap(), output);
    link.handle().set_trust(node.id(), false).await.unwrap();
    assert!(link.handle().peers().await.unwrap().is_empty());
    assert_eq!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    drop(client);
    link.close().await.unwrap();
    let link = Link::controller(&path, NetworkScope::default())
        .await
        .unwrap();
    assert!(link.handle().peers().await.unwrap().is_empty());
    link.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_existing_profiles() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    node.shutdown().await.unwrap();
    assert!(
        Link::controller(&profile, NetworkScope::default())
            .await
            .is_err()
    );
    let profile = directory.path().join("controller");
    let link = Link::controller(&profile, NetworkScope::default())
        .await
        .unwrap();
    link.close().await.unwrap();
    std::fs::write(profile.join("peers.json"), b"invalid").unwrap();
    assert!(
        Link::controller(&profile, NetworkScope::default())
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(profile.join("peers.json")).unwrap(),
        b"invalid"
    );
}
