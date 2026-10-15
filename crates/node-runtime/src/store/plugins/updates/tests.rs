use super::*;
use crate::store::{CAPACITY, Store};
use sailry_protocol::{Command, NodeId};
use tokio::sync::mpsc;

async fn fixture() -> (tempfile::TempDir, Store, Ingress, mpsc::Receiver<Job>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let directory = tempfile::tempdir().unwrap();
    let store = Store::start(
        directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3"),
        NodeId([3; 32]),
        None,
        Default::default(),
        Default::default(),
        Default::default(),
        crate::store::mutations::execute,
    )
    .await
    .unwrap();
    let (sender, receiver) = mpsc::channel(CAPACITY);
    let mut ingress = (*store.ingress).clone();
    ingress.sender = sender;
    ingress.closed = sailry_link::CancellationToken::new();
    (directory, store, ingress, receiver)
}

fn fill(ingress: &Ingress) {
    for _ in 0..CAPACITY {
        let (reply, _) = oneshot::channel();
        ingress
            .sender
            .try_send(Job::PluginUpdate {
                name: "queued".into(),
                revision: 1,
                reply,
            })
            .unwrap();
    }
    assert_eq!(ingress.sender.capacity(), 0);
}

#[tokio::test]
async fn waits_for_capacity_then_recovers() {
    let (_directory, store, ingress, mut receiver) = fixture().await;
    fill(&ingress);
    let mut read = Box::pin(ingress.update_info("example", 7));
    assert!(futures::poll!(&mut read).is_pending());
    for _ in 0..CAPACITY {
        let Job::PluginUpdate { name, .. } = receiver.recv().await.unwrap() else {
            panic!("plugin update job expected");
        };
        assert_eq!(name, "queued");
    }
    assert!(futures::poll!(&mut read).is_pending());
    let Job::PluginUpdate {
        name,
        revision,
        reply,
    } = receiver.recv().await.unwrap()
    else {
        panic!("plugin update job expected");
    };
    assert_eq!(name, "example");
    assert_eq!(revision, 7);
    let error = Fault::new(ErrorCode::NotFound, "fixture package is missing");
    reply.send(Err(error.clone())).unwrap();
    assert_eq!(read.await.unwrap_err(), error);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn closed_queue_is_unavailable() {
    let (_directory, store, ingress, receiver) = fixture().await;
    drop(receiver);
    assert_eq!(
        ingress.update_info("example", 1).await.unwrap_err().code,
        ErrorCode::Unavailable
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_while_queue_is_full() {
    let (_directory, store, ingress, mut receiver) = fixture().await;
    fill(&ingress);
    let mut read = Box::pin(ingress.update_info("example", 1));
    assert!(futures::poll!(&mut read).is_pending());
    ingress.closed.cancel();
    assert_eq!(read.await.unwrap_err().code, ErrorCode::Unavailable);
    for _ in 0..CAPACITY {
        let Job::PluginUpdate { name, .. } = receiver.recv().await.unwrap() else {
            panic!("plugin update job expected");
        };
        assert_eq!(name, "queued");
    }
    assert!(receiver.try_recv().is_err());
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_while_awaiting_reply() {
    let (_directory, store, ingress, mut receiver) = fixture().await;
    let mut read = Box::pin(ingress.update_info("example", 1));
    assert!(futures::poll!(&mut read).is_pending());
    let job = receiver.recv().await.unwrap();
    ingress.closed.cancel();
    assert_eq!(read.await.unwrap_err().code, ErrorCode::Unavailable);
    let Job::PluginUpdate { reply, .. } = job else {
        panic!("plugin update job expected");
    };
    assert!(reply.is_closed());
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn authorization_distinguishes_full_and_closed() {
    let (_directory, store, ingress, receiver) = fixture().await;
    fill(&ingress);
    let request = Request::new(
        ingress.node,
        Command::CheckPluginUpdates {
            force: false,
            packages: Vec::new(),
        },
    );
    let full = ingress
        .check_plugin(ingress.node, request.clone())
        .await
        .unwrap_err();
    assert_eq!(full.code, ErrorCode::Busy);
    assert_eq!(full.message, "Node request queue is full");
    drop(receiver);
    assert_eq!(
        ingress
            .check_plugin(ingress.node, request)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
    store.shutdown().await.unwrap();
}
