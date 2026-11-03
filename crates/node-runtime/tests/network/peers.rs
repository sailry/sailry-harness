use std::{collections::BTreeSet, time::Duration};

use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, NodeId, Output, Update};
use tokio::sync::watch;

struct Fixture {
    _directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Client,
    peer: NodeId,
}

impl Fixture {
    async fn new(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller = Link::controller(directory.path().join("phone"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let peer = if remote {
            NodeId(*controller.handle().address().id.as_bytes())
        } else {
            node.id()
        };
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        Self {
            _directory: directory,
            node,
            controller,
            client,
            peer,
        }
    }
}

async fn wait(peers: &mut watch::Receiver<BTreeSet<NodeId>>, expected: BTreeSet<NodeId>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if *peers.borrow_and_update() == expected {
                return;
            }
            peers.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn follows_observer_lifecycle() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let mut peers = fixture.node.link().connected_peers().unwrap();
        let active = BTreeSet::from([fixture.peer]);
        let phone = fixture.controller.handle().address();
        assert!(
            fixture
                .node
                .link()
                .inspect(&phone)
                .await
                .unwrap()
                .compatibility()
                .is_ok()
        );
        assert!(peers.borrow().is_empty());
        assert!(matches!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::Snapshot))
                .await
                .unwrap(),
            Output::Snapshot(_)
        ));
        assert!(peers.borrow().is_empty());

        let mut first = fixture.client.subscribe().await.unwrap();
        assert!(matches!(first.next().await.unwrap(), Update::Snapshot(_)));
        wait(&mut peers, active.clone()).await;
        let mut second = fixture.client.subscribe().await.unwrap();
        assert!(matches!(second.next().await.unwrap(), Update::Snapshot(_)));
        drop(first);
        assert_eq!(*peers.borrow(), active);
        drop(second);
        wait(&mut peers, BTreeSet::new()).await;

        assert!(fixture.node.link().inspect(&phone).await.is_ok());
        assert!(peers.borrow().is_empty());
        let mut reconnected = fixture.client.subscribe().await.unwrap();
        assert!(matches!(
            reconnected.next().await.unwrap(),
            Update::Snapshot(_)
        ));
        wait(&mut peers, active).await;
        if remote {
            fixture
                .node
                .link()
                .set_trust(fixture.peer, false)
                .await
                .unwrap();
        } else {
            drop(reconnected);
        }
        wait(&mut peers, BTreeSet::new()).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn shutdown_clears_retained_observers() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let mut peers = fixture.node.link().connected_peers().unwrap();
        let mut retained = fixture.client.subscribe().await.unwrap();
        assert!(matches!(
            retained.next().await.unwrap(),
            Update::Snapshot(_)
        ));
        wait(&mut peers, BTreeSet::from([fixture.peer])).await;
        fixture.node.shutdown().await.unwrap();
        assert!(peers.borrow().is_empty());
        assert!(retained.next().await.is_err());
        drop(retained);
        fixture.controller.close().await.unwrap();
    }
}
