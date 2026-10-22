use super::*;
use sailry_protocol::{Defaults, MAX_FRAME_BYTES};

fn snapshot() -> Snapshot {
    Snapshot {
        node: NodeId([1; 32]),
        cursor: 7,
        defaults: Defaults {
            revision: 0,
            config: None,
        },
        projects: vec![],
        worktrees: vec![],
        sessions: vec![],
        terminals: vec![],
        terminal_settings_revision: 0,
        power_settings_revision: 0,
        sandbox_settings_revision: 0,
        search_settings_revision: 0,
        media_settings: Default::default(),
        providers: vec![],
        provider_quotas: vec![],
        model_catalog: Default::default(),
        roles: vec![],
        ssh: vec![],
        databases: vec![],
        plugins: vec![],
        notifications: vec![],
        turns: vec![],
    }
}

fn header(delivery: Delivery, snapshot: &Snapshot, bytes: &str) -> Header {
    Header {
        delivery,
        node: snapshot.node,
        cursor: snapshot.cursor,
        bytes: bytes.len() as u64,
    }
}

fn page(index: u64, data: &str) -> Reply {
    Reply::SnapshotPage(Page {
        index,
        data: data.into(),
    })
}

#[test]
fn bounds_escaped_pages() {
    let page = page(0, &"\0".repeat(PAGE_BYTES));
    let bytes = serde_json::to_vec(&page).unwrap();
    assert!(bytes.len() < MAX_FRAME_BYTES);
    assert!(super::super::length(bytes.len()).is_ok());
}

#[test]
fn exposes_complete_delivery() {
    let snapshot = snapshot();
    let bytes = serde_json::to_string(&snapshot).unwrap();
    for delivery in [Delivery::Completed, Delivery::Update] {
        let mut assembly = Assembly::default();
        assert!(
            assembly
                .accept(Reply::SnapshotStart(header(delivery, &snapshot, &bytes)))
                .unwrap()
                .is_none()
        );
        assert!(assembly.accept(page(0, &bytes[..100])).unwrap().is_none());
        let result = assembly.accept(page(1, &bytes[100..])).unwrap().unwrap();
        match (delivery, result) {
            (Delivery::Completed, Reply::Completed(response)) => {
                assert_eq!(*response, Ok(Output::Snapshot(snapshot.clone())));
            }
            (Delivery::Update, Reply::Update(update)) => {
                assert_eq!(*update, Update::Snapshot(snapshot.clone()));
            }
            _ => panic!("snapshot delivery expected"),
        }
        assert!(assembly.pending.is_none());
    }
}

#[test]
fn rejects_invalid_sequence() {
    let snapshot = snapshot();
    let bytes = serde_json::to_string(&snapshot).unwrap();
    let header = header(Delivery::Update, &snapshot, &bytes);
    assert!(Assembly::default().accept(page(0, &bytes)).is_err());
    let cases = [
        Reply::SnapshotStart(header.clone()),
        page(0, ""),
        page(0, &"x".repeat(PAGE_BYTES + 1)),
        page(0, &format!("{bytes}x")),
        page(1, &bytes),
        Reply::Update(Box::new(Update::ResetRequired)),
    ];
    for reply in cases {
        let mut assembly = Assembly::default();
        assembly
            .accept(Reply::SnapshotStart(header.clone()))
            .unwrap();
        assert!(assembly.accept(reply).is_err());
    }
    let mut header = header;
    header.bytes = 0;
    assert!(
        Assembly::default()
            .accept(Reply::SnapshotStart(header))
            .is_err()
    );
}

#[test]
fn checks_snapshot_identity() {
    let snapshot = snapshot();
    let bytes = serde_json::to_string(&snapshot).unwrap();
    for change_node in [false, true] {
        let mut header = header(Delivery::Update, &snapshot, &bytes);
        if change_node {
            header.node = NodeId([2; 32]);
        } else {
            header.cursor += 1;
        }
        let mut assembly = Assembly::default();
        assembly.accept(Reply::SnapshotStart(header)).unwrap();
        assert!(assembly.accept(page(0, &bytes)).is_err());
    }
}

#[test]
fn does_not_allocate_declared_size() {
    let snapshot = snapshot();
    let mut header = header(Delivery::Update, &snapshot, "x");
    header.bytes = u64::MAX;
    let mut assembly = Assembly::default();
    assembly.accept(Reply::SnapshotStart(header)).unwrap();
    assert_eq!(assembly.pending.as_ref().unwrap().bytes.capacity(), 0);
    assert!(assembly.accept(page(0, "x")).unwrap().is_none());
    assert_eq!(assembly.pending.as_ref().unwrap().bytes.as_slice(), b"x");
}

#[tokio::test]
async fn cancellation_preserves_pages() {
    use iroh::{Endpoint, RelayMode, endpoint::presets};
    use std::time::Duration;

    let server = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .alpns(vec![b"snapshot-test".to_vec()])
        .bind()
        .await
        .unwrap();
    let client = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .bind()
        .await
        .unwrap();
    let (outgoing, incoming) =
        tokio::join!(client.connect(server.addr(), b"snapshot-test"), async {
            server.accept().await.unwrap().await.unwrap()
        });
    let outgoing = outgoing.unwrap();
    let (mut send, _) = outgoing.open_bi().await.unwrap();
    let snapshot = snapshot();
    let bytes = serde_json::to_string(&snapshot).unwrap();
    super::super::write(
        &mut send,
        &Reply::SnapshotStart(header(Delivery::Update, &snapshot, &bytes)),
    )
    .await
    .unwrap();
    super::super::write(&mut send, &page(0, &bytes[..100]))
        .await
        .unwrap();
    let (_, mut recv) = incoming.accept_bi().await.unwrap();
    let mut reader = super::super::Reader::default();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), reader.reply(&mut recv))
            .await
            .is_err()
    );
    super::super::write(&mut send, &page(1, &bytes[100..]))
        .await
        .unwrap();
    send.finish().unwrap();
    let Reply::Update(update) = reader.reply(&mut recv).await.unwrap() else {
        panic!("snapshot update expected")
    };
    assert_eq!(*update, Update::Snapshot(snapshot));
    client.close().await;
    server.close().await;
}
