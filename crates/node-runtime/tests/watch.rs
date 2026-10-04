use sailry_client::{Client, View};
use sailry_link::CancellationToken;
use sailry_node_runtime::{NetworkScope, Node};
use sailry_protocol::{Command, ErrorCode};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

async fn wait(updates: &mut watch::Receiver<View>, check: impl Fn(&View) -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if check(&updates.borrow_and_update()) {
                return;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn shares_projection_until_cancelled() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let host = Node::start(directory.path().join("host")).await.unwrap();
        let controller = Node::start(directory.path().join("controller"))
            .await
            .unwrap();
        host.link().set_trust(controller.id(), true).await.unwrap();
        controller.link().set_trust(host.id(), true).await.unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.link().remote(host.link().address())
        } else {
            host.local()
        }));
        let stop = CancellationToken::new();
        let (sender, mut updates) = watch::channel(View::default());
        let run = {
            let client = client.clone();
            let stop = stop.clone();
            tokio::spawn(async move { client.watch(sender, stop).await })
        };
        wait(&mut updates, |view| view.connected).await;
        client
            .execute(client.prepare(Command::RegisterProject {
                name: "Observed".into(),
                path: directory.path().to_str().unwrap().into(),
            }))
            .await
            .unwrap();
        wait(&mut updates, |view| {
            view.snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.projects.len() == 1)
        })
        .await;
        assert_eq!(updates.borrow().snapshot.as_ref().unwrap().node, host.id());
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(1), run)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!updates.borrow().connected);
        host.shutdown().await.unwrap();
        controller.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn recovers_and_revokes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("host");
    let host = Node::start(&path).await.unwrap();
    let address = host.link().address();
    let socket = *address.ip_addrs().next().unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    host.link().set_trust(controller.id(), true).await.unwrap();
    controller.link().set_trust(host.id(), true).await.unwrap();
    let client = Client::new(controller.link().remote(address));
    let (sender, mut updates) = watch::channel(View::default());
    let run = tokio::spawn(async move { client.watch(sender, CancellationToken::new()).await });
    wait(&mut updates, |view| view.connected).await;
    host.shutdown().await.unwrap();
    wait(&mut updates, |view| !view.connected && view.error.is_some()).await;
    let host = Node::start_with_network(&path, NetworkScope::Direct(socket))
        .await
        .unwrap();
    wait(&mut updates, |view| view.connected && view.error.is_none()).await;
    host.link().set_trust(controller.id(), false).await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(!updates.borrow().connected);
    host.shutdown().await.unwrap();
    controller.shutdown().await.unwrap();
}
