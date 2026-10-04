use sailry_link::{Link, NetworkScope};
use sailry_protocol::NodeId;

#[tokio::test]
async fn requires_pairing() {
    let directory = tempfile::tempdir().unwrap();
    let first = Link::controller(directory.path().join("first"), NetworkScope::default())
        .await
        .unwrap();
    let second = Link::controller(directory.path().join("second"), NetworkScope::default())
        .await
        .unwrap();
    let address = first.handle().address();
    first.handle().set_name("Test phone".into()).unwrap();
    assert!(second.handle().inspect(&address).await.is_err());
    let invitation = first.handle().invite().unwrap();
    second.handle().pair(invitation.ticket()).await.unwrap();
    let info = second.handle().inspect(&address).await.unwrap();
    assert_eq!(info.name.as_deref(), Some("Test phone"));
    assert_eq!(info.platform, std::env::consts::OS);
    assert!(!info.execution);
    first.handle().set_name("Renamed phone".into()).unwrap();
    assert_eq!(
        second
            .handle()
            .inspect(&address)
            .await
            .unwrap()
            .name
            .as_deref(),
        Some("Renamed phone")
    );
    assert!(first.handle().set_name("line\nbreak".into()).is_err());
    first
        .handle()
        .set_trust(NodeId(*second.handle().address().id.as_bytes()), false)
        .await
        .unwrap();
    assert!(second.handle().inspect(&address).await.is_err());
    first.close().await.unwrap();
    second.close().await.unwrap();
}
