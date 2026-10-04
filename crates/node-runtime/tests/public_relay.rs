//! Opt-in public relay acceptance with direct IP transports disabled at the peer.
use iroh::{
    Endpoint,
    endpoint::{RecvStream, SendStream, presets},
};
use sailry_node_runtime::{NetworkScope, Node};
use sailry_protocol::{Command, MAX_FRAME_BYTES, Request};
use serde_json::{Value, json};

async fn write(send: &mut SendStream, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .unwrap();
    send.write_all(&bytes).await.unwrap();
    send.finish().unwrap();
}
async fn read(recv: &mut RecvStream) -> Value {
    let mut length = [0; 4];
    recv.read_exact(&mut length).await.unwrap();
    let length = u32::from_be_bytes(length) as usize;
    assert!((1..=MAX_FRAME_BYTES).contains(&length));
    let mut bytes = vec![0; length];
    recv.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
#[ignore = "contacts public n0 relays using isolated temporary identities"]
async fn routes_without_direct_connections() {
    tokio::time::timeout(std::time::Duration::from_secs(45), run())
        .await
        .expect("public relay deadline");
}

#[tokio::test]
#[ignore = "contacts public n0 relays using isolated temporary identities"]
async fn recovers_restarted_endpoint() {
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        use sailry_client::{Client, View};
        use sailry_link::{CancellationToken, Link};
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_with_network(&profile, NetworkScope::Internet)
            .await
            .unwrap();
        node.link().online().await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::Internet)
                .await
                .unwrap();
        controller.handle().online().await.unwrap();
        let invitation = node.link().invite().unwrap();
        let previous = controller.handle().pair(invitation.ticket()).await.unwrap();
        let client = Client::new(controller.handle().remote(previous.clone()));
        let stop = CancellationToken::new();
        let (sender, mut receiver) = tokio::sync::watch::channel(View::default());
        let cancellation = stop.clone();
        let watch = tokio::spawn(async move { client.watch(sender, cancellation).await });
        receiver.wait_for(|view| view.connected).await.unwrap();
        node.shutdown().await.unwrap();
        receiver.wait_for(|view| !view.connected).await.unwrap();
        let node = Node::start_with_network(&profile, NetworkScope::Internet)
            .await
            .unwrap();
        node.link().online().await.unwrap();
        assert_ne!(
            previous.addrs,
            node.link().address().addrs,
            "restart changed endpoint addresses"
        );
        let recovered = receiver
            .wait_for(|view| view.connected)
            .await
            .unwrap()
            .clone();
        assert_eq!(recovered.snapshot.unwrap().node, node.id());
        stop.cancel();
        watch.await.unwrap().unwrap();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    })
    .await
    .expect("public restart deadline");
}

async fn run() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start_with_network(directory.path().join("node"), NetworkScope::Internet)
        .await
        .unwrap();
    node.link().online().await.unwrap();
    let peer = Endpoint::builder(presets::N0)
        .clear_ip_transports()
        .bind()
        .await
        .unwrap();
    peer.online().await;
    let invitation = node.link().invite().unwrap();
    let ticket: Value = serde_json::from_str(invitation.ticket()).unwrap();
    let pairing = peer
        .connect(node.link().address(), b"sailry/pair/1")
        .await
        .unwrap();
    let (mut send, mut recv) = pairing.open_bi().await.unwrap();
    write(
        &mut send,
        json!({"token": ticket["token"], "address": peer.addr()}),
    )
    .await;
    assert_eq!(read(&mut recv).await, json!({"Ok": null}));
    pairing.close(0u32.into(), b"pairing complete");

    let connection = peer
        .connect(node.link().address(), b"sailry/node/1")
        .await
        .unwrap();
    let paths = connection.paths();
    assert!(paths.iter().any(|path| path.is_relay()));
    assert!(!paths.iter().any(|path| path.is_ip()));
    let request = Request::new(
        node.id(),
        Command::RegisterProject {
            name: "Public relay fixture".into(),
            path: directory.path().to_str().unwrap().into(),
        },
    );
    let mut first = None;
    for _ in 0..2 {
        let (mut send, mut recv) = connection.open_bi().await.unwrap();
        write(&mut send, json!({"kind": "command", "data": request})).await;
        let receipt = read(&mut recv).await;
        assert_eq!(receipt["kind"], "receipt");
        assert_eq!(receipt["data"]["durable"], true);
        let completed = read(&mut recv).await;
        assert_eq!(completed["data"]["Ok"]["kind"], "project");
        if let Some(first) = &first {
            assert_eq!(&completed, first);
        }
        first = Some(completed);
    }
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    write(&mut send, json!({"kind": "subscribe"})).await;
    let snapshot = read(&mut recv).await;
    assert_eq!(snapshot["kind"], "update");
    assert_eq!(snapshot["data"]["kind"], "snapshot");
    assert_eq!(
        snapshot["data"]["data"]["projects"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    connection.close(0u32.into(), b"test complete");
    peer.close().await;
    node.shutdown().await.unwrap();
}
