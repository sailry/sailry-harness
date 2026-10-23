use super::*;
use crate::store::{CAPACITY, Store};
use std::{collections::BTreeSet, time::Duration};
use tokio::sync::mpsc;

#[tokio::test]
async fn admits_unrelated_resources_until_shutdown() {
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
    let (sender, mut receiver) = mpsc::channel(CAPACITY);
    let mut ingress = (*store.ingress).clone();
    ingress.sender = sender;
    ingress.closed = sailry_link::CancellationToken::new();
    let ingress = Arc::new(ingress);
    let mut worker = Worker::new(ingress.clone());
    let mut expected = BTreeSet::new();
    let execution = || {
        let (reply, _) = oneshot::channel();
        Execution {
            caller: ingress.node,
            request: Request::new(
                ingress.node,
                Command::CloseTerminal {
                    worktree: WorktreeId::new(),
                    terminal: TerminalId::new(),
                },
            ),
            reply,
        }
    };
    // Missing targets exercise the actual owner without launching a terminal.
    for _ in 0..CAPACITY + 4 {
        let execution = execution();
        expected.insert(execution.request.id);
        assert!(worker.submit(execution).is_ok());
    }
    assert_eq!(worker.pending, expected.len());
    tokio::time::timeout(Duration::from_secs(10), async {
        while !expected.is_empty() {
            let Job::Resources(completed) = receiver.recv().await.unwrap() else {
                panic!("resource completion expected")
            };
            assert!(expected.remove(&completed.request.id));
            assert_eq!(completed.result.unwrap_err().code, ErrorCode::NotFound);
            worker.pending -= 1;
        }
    })
    .await
    .expect("independent resource completion deadline");
    assert_eq!(worker.pending, 0);
    ingress.closed.cancel();
    assert!(worker.submit(execution()).is_err());
    store.shutdown().await.unwrap();
}
