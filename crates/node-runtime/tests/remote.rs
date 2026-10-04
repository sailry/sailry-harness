use std::time::Duration;

use sailry_client::{Apply, Client, Projection};
use sailry_node_runtime::Node;
use sailry_protocol::*;

async fn trusted_pair() -> (tempfile::TempDir, Node, Node, Client) {
    let directory = tempfile::tempdir().unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    let execution = Node::start(directory.path().join("execution"))
        .await
        .unwrap();
    controller
        .link()
        .set_trust(execution.id(), true)
        .await
        .unwrap();
    execution
        .link()
        .set_trust(controller.id(), true)
        .await
        .unwrap();
    let client = Client::new(controller.link().remote(execution.link().address()));
    (directory, controller, execution, client)
}

#[tokio::test]
async fn routes_to_execution_node() {
    let (directory, controller, execution, client) = trusted_pair().await;
    let mut subscription = client.subscribe().await.unwrap();
    let mut projection = Projection::new(execution.id(), 1);
    projection
        .apply(1, subscription.next().await.unwrap())
        .unwrap();
    let request = client.prepare(Command::RegisterProject {
        name: "Remote project".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let admission = client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    let original = admission.completion.await.unwrap().unwrap();
    assert_eq!(client.execute(request.clone()).await.unwrap(), original);
    assert_eq!(
        projection
            .apply(1, subscription.next().await.unwrap())
            .unwrap(),
        Apply::Recover
    );
    client.recover(&mut projection, 1).await.unwrap();
    let Output::Snapshot(state) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert_eq!(projection.snapshot().unwrap(), &state);
    assert_eq!(state.projects.len(), 1);
    let local = Client::new(controller.local());
    let Output::Snapshot(state) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert!(state.projects.is_empty());
    drop(subscription);
    controller.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_unpaired_reads() {
    let directory = tempfile::tempdir().unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    let execution = Node::start(directory.path().join("execution"))
        .await
        .unwrap();
    let client = Client::new(controller.link().remote(execution.link().address()));
    assert_eq!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    controller
        .link()
        .set_trust(execution.id(), true)
        .await
        .unwrap();
    // Outgoing trust is not sufficient: the execution Node must authorize the caller too.
    assert!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .is_err()
    );
    controller.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn restores_pairing_and_requests() {
    let (directory, controller, execution, client) = trusted_pair().await;
    let request = client.prepare(Command::RegisterProject {
        name: "Remote".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let original = client.execute(request.clone()).await.unwrap();
    let controller_path = controller.profile().to_owned();
    let execution_path = execution.profile().to_owned();
    controller.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
    let controller = Node::start(controller_path).await.unwrap();
    let execution = Node::start(execution_path).await.unwrap();
    let client = Client::new(controller.link().remote(execution.link().address()));
    assert_eq!(client.execute(request).await.unwrap(), original);
    controller.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn revokes_active_peer() {
    let (_directory, controller, execution, client) = trusted_pair().await;
    let mut subscription = client.subscribe().await.unwrap();
    subscription.next().await.unwrap();
    execution
        .link()
        .set_trust(controller.id(), false)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), subscription.next())
            .await
            .unwrap()
            .is_err()
    );
    assert!(
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .is_err()
    );
    controller.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn releases_dropped_subscriptions() {
    let (_directory, controller, execution, client) = trusted_pair().await;
    for _ in 0..24 {
        let mut subscription = client.subscribe().await.unwrap();
        subscription.next().await.unwrap();
        drop(subscription);
    }
    client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap();
    controller.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_received_mutations() {
    let (directory, controller, execution, client) = trusted_pair().await;
    let request = client.prepare(Command::RegisterProject {
        name: "Durable".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let admission = client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    controller.link().disconnect(execution.id()).await;
    drop(admission);
    client.execute(request).await.unwrap();
    let Output::Snapshot(state) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert_eq!(state.projects.len(), 1);
    controller.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
}
