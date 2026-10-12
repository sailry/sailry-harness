use super::*;
use crate::store::{CAPACITY, Store, mutations::Roots};
use sailry_protocol::{
    Command, NodeId, Output, Request,
    installation::{Progress, State},
    models::Kind,
};
use std::{path::PathBuf, time::Duration};
use tokio::sync::mpsc;

async fn fixture() -> (tempfile::TempDir, Store, Arc<Ingress>, mpsc::Receiver<Job>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let directory = tempfile::tempdir().unwrap();
    let store = Store::start(
        directory.path().join("node.sqlite3"),
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
    ingress.closed = CancellationToken::new();
    (directory, store, Arc::new(ingress), receiver)
}

fn mutation(node: NodeId) -> Mutation {
    let (reply, _) = oneshot::channel();
    Mutation {
        caller: node,
        request: Request::new(
            node,
            Command::InstallBundledPlugin {
                name: "fixture".into(),
                expected_revision: 0,
            },
        ),
        roots: Roots {
            source: PathBuf::new(),
            target: PathBuf::new(),
        },
        reply,
    }
}

fn plugin(name: &str) -> Resource {
    Resource::Plugin { name: name.into() }
}

async fn installed(receiver: &mut mpsc::Receiver<Job>) -> (Box<Completed>, oneshot::Sender<()>) {
    let Job::Installed { completed, release } =
        tokio::time::timeout(Duration::from_secs(3), receiver.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("installation completion expected")
    };
    (completed, release)
}

#[tokio::test]
async fn holds_order_through_business_completion() {
    let (_directory, store, ingress, mut receiver) = fixture().await;
    let mut worker = Worker::new(ingress.clone());
    let first = mutation(ingress.node);
    let first_id = first.request.id;
    let second = mutation(ingress.node);
    let second_id = second.request.id;
    let (started, mut starting) = oneshot::channel();
    assert!(
        worker
            .submit(first, plugin("same"), move |report, _| async move {
                report(Phase::Inspecting, None);
                Ok(Output::InstallationDismissed { request: first_id })
            })
            .is_ok()
    );
    assert!(
        worker
            .submit(second, plugin("same"), move |report, _| async move {
                let _ = started.send(());
                report(Phase::Inspecting, None);
                Ok(Output::InstallationDismissed { request: second_id })
            })
            .is_ok()
    );
    let (completed, release) = installed(&mut receiver).await;
    assert_eq!(completed.request.id, first_id);
    assert_eq!(worker.pending, 2);
    assert!(starting.try_recv().is_err());
    let mut stream =
        ingress
            .installations
            .subscribe(ingress.node, ingress.node, ingress.closed.clone());
    let sailry_protocol::Update::Installations { tasks, .. } = stream.next().await.unwrap() else {
        panic!("installation snapshot expected")
    };
    assert_eq!(tasks.len(), 2);
    assert!(matches!(
        tasks[0].state,
        State::Running {
            phase: Phase::Installing,
            ..
        }
    ));
    assert_eq!(tasks[1].state, State::Queued);
    // This interface fixture replaces only the Store's final business-commit callback.
    ingress
        .installations
        .finish(completed.caller, completed.request.id, &completed.result);
    worker.pending -= 1;
    release.send(()).unwrap();
    starting.await.unwrap();
    let (completed, release) = installed(&mut receiver).await;
    assert_eq!(completed.request.id, second_id);
    ingress
        .installations
        .finish(completed.caller, completed.request.id, &completed.result);
    worker.pending -= 1;
    release.send(()).unwrap();
    assert_eq!(worker.pending, 0);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_capacity_without_counting_failed_rows() {
    let (_directory, store, ingress, mut receiver) = fixture().await;
    let mut worker = Worker::new(ingress.clone());
    for index in 0..9 {
        assert!(
            worker
                .submit(
                    mutation(ingress.node),
                    plugin(&format!("failed-{index}")),
                    |report, _| async move {
                        report(Phase::Inspecting, None);
                        Err(Fault::invalid("fixture failure"))
                    }
                )
                .is_ok()
        );
        let (completed, release) = installed(&mut receiver).await;
        ingress
            .installations
            .finish(completed.caller, completed.request.id, &completed.result);
        worker.pending -= 1;
        release.send(()).unwrap();
    }
    assert_eq!(worker.pending, 0);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    for index in 0..8 {
        let gate = gate.clone();
        assert!(worker.submit(mutation(ingress.node), plugin(&format!("pending-{index}")), move |report, stop| async move {
            report(Phase::Preparing, None);
            tokio::select! {
                _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "fixture stopped")),
                _ = gate.acquire() => unreachable!("fixture gate remains closed"),
            }
        }).is_ok());
    }
    assert_eq!(worker.pending, 8);
    let rejected = worker
        .submit(
            mutation(ingress.node),
            plugin("over-capacity"),
            |_, _| async { unreachable!("rejected preparation must not run") },
        )
        .err()
        .unwrap();
    ingress.installations.finish(
        rejected.caller,
        rejected.request.id,
        &Err(Fault::new(ErrorCode::Busy, "fixture capacity")),
    );
    assert_eq!(worker.pending, 8);
    ingress.closed.cancel();
    for _ in 0..8 {
        let (completed, release) = installed(&mut receiver).await;
        ingress
            .installations
            .finish(completed.caller, completed.request.id, &completed.result);
        worker.pending -= 1;
        release.send(()).unwrap();
    }
    assert_eq!(worker.pending, 0);
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn accepts_a_model_service_callback() {
    let (_directory, store, ingress, mut receiver) = fixture().await;
    let mut worker = Worker::new(ingress.clone());
    let mutation = mutation(ingress.node);
    let id = mutation.request.id;
    let (release, waiting) = oneshot::channel();
    let (reported, report_ready) = oneshot::channel();
    let progress = Progress::Percent {
        percent: 35,
        bytes_per_second: Some(100),
    };
    // Public substitution demonstrates the common adapter interface, not model integration.
    assert!(
        worker
            .submit(
                mutation,
                Resource::Model {
                    kind: Kind::Embedding,
                    id: "fixture-model".into()
                },
                move |report, _| async move {
                    report(Phase::Downloading, Some(progress));
                    reported.send(()).unwrap();
                    waiting.await.unwrap();
                    report(Phase::Validating, None);
                    Ok(Output::InstallationDismissed { request: id })
                }
            )
            .is_ok()
    );
    report_ready.await.unwrap();
    let mut stream =
        ingress
            .installations
            .subscribe(ingress.node, ingress.node, ingress.closed.clone());
    let sailry_protocol::Update::Installations { tasks, .. } = stream.next().await.unwrap() else {
        panic!("installation snapshot expected")
    };
    assert_eq!(
        tasks[0].state,
        State::Running {
            phase: Phase::Downloading,
            progress: Some(progress)
        }
    );
    release.send(()).unwrap();
    let (completed, committed) = installed(&mut receiver).await;
    ingress
        .installations
        .finish(completed.caller, completed.request.id, &completed.result);
    worker.pending -= 1;
    committed.send(()).unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_keeps_its_queue_position() {
    let (committed, preceding) = oneshot::channel();
    let stop = CancellationToken::new();
    stop.cancel();
    let mut queued = Box::pin(wait(Some(preceding), &stop));
    assert!(queued.as_mut().now_or_never().is_none());
    committed.send(()).unwrap();
    assert_eq!(queued.await.unwrap_err().code, ErrorCode::Cancelled);
}

#[tokio::test]
async fn cleanup_preserves_pending_predecessors() {
    let mut preceding = BTreeMap::new();
    let (committed, completed) = oneshot::channel();
    committed.send(()).unwrap();
    preceding.insert(plugin("completed"), completed);
    let (abandoned, closed) = oneshot::channel();
    drop(abandoned);
    preceding.insert(plugin("closed"), closed);
    let (committed, pending) = oneshot::channel();
    preceding.insert(plugin("pending"), pending);
    retain(&mut preceding);
    assert_eq!(preceding.len(), 1);
    committed.send(()).unwrap();
    preceding.remove(&plugin("pending")).unwrap().await.unwrap();
}
