use super::*;
use crate::store::{CONTROL_CAPACITY, Store};
use sailry_protocol::{Command, NodeId};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinHandle};

async fn fixture() -> (tempfile::TempDir, Store, Arc<Ingress>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let directory = tempfile::tempdir().unwrap();
    let store = Store::start(
        directory.path().canonicalize().unwrap().join("node.turso"),
        NodeId([3; 32]),
        None,
        Default::default(),
        Default::default(),
        Default::default(),
        crate::store::mutations::execute,
    )
    .await
    .unwrap();
    let ingress = store.ingress.clone();
    (directory, store, ingress)
}

struct Control {
    release: Option<std::sync::mpsc::Sender<()>>,
    pending: Vec<JoinHandle<()>>,
}

impl Control {
    async fn finish(mut self) {
        self.release.take().unwrap().send(()).unwrap();
        for task in std::mem::take(&mut self.pending) {
            task.await.unwrap();
        }
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

async fn blocked(ingress: &Arc<Ingress>) -> Control {
    let (entered, mut waiting) = mpsc::unbounded_channel();
    let (release, released) = std::sync::mpsc::channel();
    ingress
        .sender
        .send(Job::Barrier {
            entered,
            release: released,
        })
        .await
        .unwrap();
    waiting.recv().await.unwrap();
    let mut control = Control {
        release: Some(release),
        pending: Vec::with_capacity(CONTROL_CAPACITY),
    };
    for _ in 0..CONTROL_CAPACITY {
        let ingress = ingress.clone();
        control.pending.push(tokio::spawn(async move {
            assert!(
                ingress
                    .claim_run(sailry_link::CancellationToken::new())
                    .await
                    .unwrap()
                    .is_none()
            );
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while ingress.sender.capacity() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("control requests did not fill the blocked owner");
    control
}

fn request(ingress: &Ingress) -> Request {
    Request::new(
        ingress.node,
        Command::CheckPluginUpdates {
            force: false,
            packages: Vec::new(),
        },
    )
}

#[tokio::test]
async fn reads_bypass_blocked_control() {
    let (_directory, store, ingress) = fixture().await;
    let control = blocked(&ingress).await;
    let result = tokio::time::timeout(Duration::from_secs(2), ingress.update_info("example", 7))
        .await
        .expect("plugin inspection waited for the control owner");
    assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
    control.finish().await;
    assert_eq!(
        ingress.update_info("example", 7).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn waits_for_storage_then_recovers() {
    let (_directory, store, ingress) = fixture().await;
    let storage = ingress.storage.get().unwrap().clone();
    let paused = storage.handle.exclusive().await.unwrap();
    let mut read = Box::pin(ingress.update_info("example", 7));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut read)
            .await
            .is_err()
    );
    drop(paused);
    let result = tokio::time::timeout(Duration::from_secs(2), read)
        .await
        .expect("plugin inspection did not resume after storage maintenance");
    assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_while_waiting_for_storage() {
    let (_directory, store, ingress) = fixture().await;
    let storage = ingress.storage.get().unwrap().clone();
    let paused = storage.handle.exclusive().await.unwrap();
    let mut read = Box::pin(ingress.update_info("example", 1));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut read)
            .await
            .is_err()
    );
    store.stop_admission();
    let result = tokio::time::timeout(Duration::from_secs(2), read)
        .await
        .expect("plugin inspection ignored service cancellation");
    assert_eq!(result.unwrap_err().code, ErrorCode::Unavailable);
    drop(paused);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn closed_service_is_unavailable() {
    let (_directory, store, ingress) = fixture().await;
    store.shutdown().await.unwrap();
    assert_eq!(
        ingress.update_info("example", 1).await.unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(
        ingress
            .check_plugin(ingress.node, request(&ingress))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
}

#[tokio::test]
async fn authorization_bypasses_blocked_control() {
    let (_directory, store, ingress) = fixture().await;
    let request = request(&ingress);
    let scoped = request.clone().with_plugin(plugin::Context {
        invocation: None,
        turn: None,
        surface: plugin::desktop::Surface::Workspace,
        package: plugin::Reference {
            name: "example".into(),
            settings_revision: 1,
            digest: "a".repeat(64),
        },
        worktree: None,
        session: None,
    });
    let denied = ingress
        .check_plugin(ingress.node, scoped.clone())
        .await
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    let control = blocked(&ingress).await;
    tokio::time::timeout(
        Duration::from_secs(2),
        ingress.check_plugin(ingress.node, request),
    )
    .await
    .expect("controller authorization waited for the control owner")
    .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        ingress.check_plugin(ingress.node, scoped.clone()),
    )
    .await
    .expect("plugin authorization waited for the control owner");
    assert_eq!(result.unwrap_err(), denied);
    control.finish().await;
    assert_eq!(
        ingress
            .check_plugin(ingress.node, scoped)
            .await
            .unwrap_err(),
        denied
    );
    store.shutdown().await.unwrap();
}
