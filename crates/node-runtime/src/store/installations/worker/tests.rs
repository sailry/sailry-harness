use super::*;
use crate::store::{CAPACITY, Store};
use sailry_protocol::{
    Command, NodeId, Output, Request,
    installation::{Progress, State},
    models::Kind,
};
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::test]
async fn orders_language_removal_without_installation_presentation() {
    let (_directory, store, ingress, mut receiver) = fixture().await;
    let mut worker = Worker::new(ingress.clone());
    let mut first = mutation(ingress.node);
    first.request.command = Command::InstallCodeServer {
        install: sailry_protocol::code::Install {
            package: "same".into(),
            version: "1".into(),
            purl: "pkg:github/fixture/server@1".into(),
            urls: Vec::new(),
            licenses: Vec::new(),
            command: None,
            revision: "a".repeat(64),
        },
    };
    let first_id = first.request.id;
    let mut removal = mutation(ingress.node);
    removal.observe = false;
    removal.request.command = Command::UninstallCodeServer {
        package: "same".into(),
        version: "2".into(),
    };
    let remove_id = removal.request.id;
    let (started, mut starting) = oneshot::channel();
    let resource = |version: &str| Resource::Language {
        package: "same".into(),
        version: version.into(),
    };
    assert!(
        worker
            .submit(first, resource("1"), move |_, _| async move {
                Ok(Output::InstallationDismissed { request: first_id })
            })
            .is_ok()
    );
    assert!(
        worker
            .submit(removal, resource("2"), move |report, _| async move {
                let _ = started.send(());
                report(Phase::Installing, None);
                Ok(Output::CodeUninstalled {
                    package: "same".into(),
                    version: "2".into(),
                })
            })
            .is_ok()
    );
    let (first, release) = installed(&mut receiver).await;
    assert_eq!(first.request.id, first_id);
    assert!(starting.try_recv().is_err());
    let tasks = ingress
        .installations
        .read(ingress.node, &[first_id, remove_id], None)
        .unwrap();
    assert_eq!(
        tasks.iter().map(|task| task.request).collect::<Vec<_>>(),
        vec![first_id]
    );
    ingress
        .installations
        .finish(first.caller, first.request.id, &first.result);
    worker.pending -= 1;
    release.send(()).unwrap();
    starting.await.unwrap();
    let (removed, release) = installed(&mut receiver).await;
    assert_eq!(removed.request.id, remove_id);
    assert_eq!(
        removed.request.command,
        Command::UninstallCodeServer {
            package: "same".into(),
            version: "2".into()
        }
    );
    assert!(
        ingress
            .installations
            .read(ingress.node, &[first_id, remove_id], None)
            .unwrap()
            .is_empty()
    );
    worker.pending -= 1;
    release.send(()).unwrap();
    store.shutdown().await.unwrap();
}

async fn fixture() -> (tempfile::TempDir, Store, Arc<Ingress>, mpsc::Receiver<Job>) {
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
    ingress.closed = CancellationToken::new();
    (directory, store, Arc::new(ingress), receiver)
}

fn mutation(node: NodeId) -> Execution {
    let (reply, _) = oneshot::channel();
    Execution {
        caller: node,
        request: Request::new(
            node,
            Command::InstallBundledPlugin {
                name: "fixture".into(),
                expected_revision: 0,
            },
        ),
        operation: false,
        observe: true,
        reply,
    }
}

fn plugin(name: &str) -> Resource {
    Resource::Plugin { name: name.into() }
}

async fn installed(receiver: &mut mpsc::Receiver<Job>) -> (Box<Completed>, oneshot::Sender<()>) {
    let Job::Installed {
        completed, release, ..
    } = tokio::time::timeout(Duration::from_secs(3), receiver.recv())
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
    preceding.insert(lane(&plugin("completed")), completed);
    let (abandoned, closed) = oneshot::channel();
    drop(abandoned);
    preceding.insert(lane(&plugin("closed")), closed);
    let (committed, pending) = oneshot::channel();
    preceding.insert(lane(&plugin("pending")), pending);
    retain(&mut preceding);
    assert_eq!(preceding.len(), 1);
    committed.send(()).unwrap();
    preceding
        .remove(&lane(&plugin("pending")))
        .unwrap()
        .await
        .unwrap();
}

#[test]
fn serializes_actual_model_families() {
    let model = |kind, id: &str| Resource::Model {
        kind,
        id: id.into(),
    };
    assert_eq!(
        lane(&model(Kind::Ocr, "first")),
        lane(&model(Kind::Ocr, "second"))
    );
    assert_ne!(
        lane(&model(Kind::Ocr, "first")),
        lane(&model(Kind::Embedding, "first"))
    );
    assert_ne!(lane(&plugin("first")), lane(&plugin("second")));
}

#[tokio::test]
async fn holds_turn_until_durable_completion() {
    use sailry_protocol::{
        SessionId, TurnId,
        plugin::{Context, Reference},
    };

    let (_directory, store, ingress, mut receiver) = fixture().await;
    let turn = TurnId::new();
    ingress
        .agents
        .register(turn, SessionId::new(), CancellationToken::new());
    let mut execution = mutation(ingress.node);
    execution.operation = true;
    execution.request.plugin = Some(Context {
        invocation: None,
        turn: Some(turn),
        surface: Default::default(),
        package: Reference {
            name: "fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        worktree: None,
        session: None,
    });
    let mut worker = Worker::new(ingress.clone());
    assert!(
        worker
            .submit(
                execution,
                Resource::Model {
                    kind: Kind::Ocr,
                    id: "fixture".into()
                },
                |report, _| async move {
                    report(Phase::Validating, None);
                    Ok(Output::OcrModel {
                        id: "fixture".into(),
                    })
                }
            )
            .is_ok()
    );
    let Job::Installed {
        completed,
        operation,
        release,
    } = receiver.recv().await.unwrap()
    else {
        panic!("installation completion expected")
    };
    assert_eq!(operation, Some(turn));
    let mut draining = Box::pin(ingress.agents.drain(turn));
    assert!(draining.as_mut().now_or_never().is_none());
    // The Store records the business outcome before ending the admitted operation.
    ingress
        .installations
        .finish(completed.caller, completed.request.id, &completed.result);
    ingress.agents.finish_operation(turn);
    draining.await;
    worker.pending -= 1;
    release.send(()).unwrap();
    store.shutdown().await.unwrap();
}
