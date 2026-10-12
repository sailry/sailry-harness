use super::*;
use sailry_protocol::{
    Command, Output,
    plugin::{Reference, desktop::Surface},
};

#[tokio::test]
async fn filters_owned_language_observations() {
    let registry = Arc::new(Registry::default());
    let caller = NodeId([1; 32]);
    let context = Context {
        invocation: None,
        turn: None,
        surface: Surface::Settings,
        package: Reference {
            name: "owner".into(),
            digest: "a".repeat(64),
            settings_revision: 1,
        },
        worktree: None,
        session: None,
    };
    let owned = request(caller).with_plugin(context.clone());
    let resource = Resource::Language {
        package: "rust-analyzer".into(),
        version: "1".into(),
    };
    registry.begin(caller, &owned, resource.clone());
    let mut other = context.clone();
    other.package.name = "other".into();
    let foreign = request(caller).with_plugin(other);
    registry.begin(caller, &foreign, resource.clone());
    let model = request(caller);
    registry.begin(
        caller,
        &model,
        Resource::Model {
            kind: sailry_protocol::models::Kind::Ocr,
            id: "private".into(),
        },
    );
    let plugin = request(caller).with_plugin(context.clone());
    registry.begin(
        caller,
        &plugin,
        Resource::Plugin {
            name: "private".into(),
        },
    );
    for id in [foreign.id, model.id, plugin.id] {
        assert_eq!(
            registry
                .read(caller, &[id], Some(&context))
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
    assert!(
        registry
            .read(NodeId([2; 32]), &[owned.id], Some(&context))
            .unwrap()
            .is_empty()
    );
    let mut subscription = registry
        .subscribe_for(
            caller,
            caller,
            vec![owned.id],
            context.clone(),
            CancellationToken::new(),
        )
        .unwrap();
    let Update::Installations { tasks, .. } = subscription.next().await.unwrap() else {
        panic!("installation snapshot expected")
    };
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].request, owned.id);
    assert_eq!(tasks[0].resource, resource);
    let error = Fault::new(ErrorCode::OutcomeUnknown, "fixture outcome is uncertain");
    registry.finish(caller, owned.id, &Err(error.clone()));
    assert_eq!(
        registry.read(caller, &[owned.id], Some(&context)).unwrap()[0].state,
        State::Uncertain { error }
    );
    registry.dismiss(caller, owned.id, Some(&context)).unwrap();
    assert!(
        registry
            .read(caller, &[owned.id], Some(&context))
            .unwrap()
            .is_empty()
    );
}

fn request(node: NodeId) -> Request {
    Request::new(
        node,
        Command::InstallBundledPlugin {
            name: "fixture".into(),
            expected_revision: 0,
        },
    )
}

async fn tasks(registry: &Registry, caller: NodeId) -> Vec<Task> {
    let Update::Installations { tasks, .. } = registry
        .subscribe(NodeId([1; 32]), caller, CancellationToken::new())
        .next()
        .await
        .unwrap()
    else {
        panic!("installation snapshot expected")
    };
    tasks
}

#[tokio::test]
async fn keeps_admission_order_and_caller_scope() {
    let registry = Arc::new(Registry::default());
    let caller = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    let first = request(caller);
    let second = request(caller);
    let foreign = request(caller);
    registry.begin(
        caller,
        &first,
        Resource::Plugin {
            name: "first".into(),
        },
    );
    registry.begin(
        other,
        &foreign,
        Resource::Plugin {
            name: "foreign".into(),
        },
    );
    registry.begin(
        caller,
        &second,
        Resource::Plugin {
            name: "second".into(),
        },
    );
    assert_eq!(
        tasks(&registry, caller)
            .await
            .iter()
            .map(|task| task.request)
            .collect::<Vec<_>>(),
        vec![first.id, second.id]
    );
    assert_eq!(tasks(&registry, other).await[0].request, foreign.id);
}

#[tokio::test]
async fn stays_active_until_business_completion() {
    let registry = Arc::new(Registry::default());
    let caller = NodeId([1; 32]);
    let request = request(caller);
    let report = registry.begin(
        caller,
        &request,
        Resource::Plugin {
            name: "fixture".into(),
        },
    );
    report(Phase::Installing, None);
    let active = tasks(&registry, caller).await;
    assert_eq!(active.len(), 1);
    assert!(matches!(
        active[0].state,
        State::Running {
            phase: Phase::Installing,
            progress: None
        }
    ));
    assert_eq!(
        registry.dismiss(caller, request.id, None).unwrap_err().code,
        ErrorCode::Busy
    );
    registry.finish(
        caller,
        request.id,
        &Ok(Output::InstallationDismissed {
            request: request.id,
        }),
    );
    assert!(tasks(&registry, caller).await.is_empty());
    report(Phase::Inspecting, None);
    assert!(tasks(&registry, caller).await.is_empty());
}

#[tokio::test]
async fn retains_failures_until_explicit_dismissal() {
    let registry = Arc::new(Registry::default());
    let caller = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    for code in [ErrorCode::InvalidRequest, ErrorCode::OutcomeUnknown] {
        let request = request(caller);
        let report = registry.begin(
            caller,
            &request,
            Resource::Plugin {
                name: "fixture".into(),
            },
        );
        let error = Fault::new(code, "fixture failure");
        registry.finish(caller, request.id, &Err(error.clone()));
        let retained = tasks(&registry, caller).await.pop().unwrap();
        assert_eq!(
            retained.state,
            if code == ErrorCode::OutcomeUnknown {
                State::Uncertain { error }
            } else {
                State::Failed { error }
            }
        );
        report(Phase::Downloading, None);
        assert_eq!(tasks(&registry, caller).await, vec![retained]);
        registry.dismiss(other, request.id, None).unwrap();
        assert_eq!(tasks(&registry, caller).await.len(), 1);
        registry.dismiss(caller, request.id, None).unwrap();
        registry.dismiss(caller, request.id, None).unwrap();
        assert!(tasks(&registry, caller).await.is_empty());
    }
}

#[tokio::test]
async fn retains_captured_provenance_privately() {
    let registry = Arc::new(Registry::default());
    let caller = NodeId([1; 32]);
    let context = Context {
        invocation: Some(RequestId::new()),
        turn: None,
        surface: Surface::Settings,
        package: Reference {
            name: "source-owner".into(),
            digest: "captured-digest".into(),
            settings_revision: 3,
        },
        worktree: None,
        session: None,
    };
    let request = request(caller).with_plugin(context.clone());
    registry.begin(
        caller,
        &request,
        Resource::Model {
            kind: sailry_protocol::models::Kind::Ocr,
            id: "fixture".into(),
        },
    );
    registry.finish(caller, request.id, &Err(Fault::invalid("fixture failure")));
    assert_eq!(
        registry.state.lock().unwrap().records[&(caller, request.id)]
            .plugin
            .as_ref(),
        Some(&context)
    );
    let wire = serde_json::to_string(&tasks(&registry, caller).await).unwrap();
    assert!(
        !wire.contains("source-owner")
            && !wire.contains("captured-digest")
            && !wire.contains("invocation")
    );
    let mut foreign = context.clone();
    foreign.invocation = Some(RequestId::new());
    assert_eq!(
        registry
            .dismiss(caller, request.id, Some(&foreign))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    registry
        .dismiss(caller, request.id, Some(&context))
        .unwrap();
}

#[tokio::test]
async fn filters_owned_model_observations() {
    let registry = Arc::new(Registry::default());
    let caller = NodeId([1; 32]);
    let context = Context {
        invocation: Some(RequestId::new()),
        turn: None,
        surface: Surface::Workspace,
        package: Reference {
            name: "owner".into(),
            digest: "a".repeat(64),
            settings_revision: 1,
        },
        worktree: None,
        session: None,
    };
    let owned = request(caller).with_plugin(context.clone());
    registry.begin(
        caller,
        &owned,
        Resource::Model {
            kind: sailry_protocol::models::Kind::Ocr,
            id: "owned".into(),
        },
    );
    let mut other_context = context.clone();
    other_context.invocation = Some(RequestId::new());
    let foreign = request(caller).with_plugin(other_context);
    registry.begin(
        caller,
        &foreign,
        Resource::Model {
            kind: sailry_protocol::models::Kind::Embedding,
            id: "foreign".into(),
        },
    );
    let package = request(caller).with_plugin(context.clone());
    registry.begin(
        caller,
        &package,
        Resource::Plugin {
            name: "package".into(),
        },
    );
    assert_eq!(
        registry
            .read(caller, &[foreign.id], Some(&context))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        registry
            .read(caller, &[package.id], Some(&context))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(
        registry
            .read(NodeId([2; 32]), &[owned.id], Some(&context))
            .unwrap()
            .is_empty()
    );
    let mut updates = registry
        .subscribe_for(
            caller,
            caller,
            vec![owned.id],
            context.clone(),
            CancellationToken::new(),
        )
        .unwrap();
    let Update::Installations { tasks, .. } = updates.next().await.unwrap() else {
        panic!("installation snapshot expected")
    };
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].request, owned.id);
    registry.finish(
        caller,
        owned.id,
        &Err(Fault::invalid("native validation failed")),
    );
    let Update::Installations { tasks, .. } = updates.next().await.unwrap() else {
        panic!("installation snapshot expected")
    };
    assert!(matches!(tasks[0].state, State::Failed { .. }));
    registry.dismiss(caller, owned.id, Some(&context)).unwrap();
    let Update::Installations { tasks, .. } = updates.next().await.unwrap() else {
        panic!("installation snapshot expected")
    };
    assert!(tasks.is_empty());
}

#[tokio::test]
async fn publishes_only_valid_measured_changes() {
    let registry = Arc::new(Registry::default());
    let caller = NodeId([1; 32]);
    let request = request(caller);
    let report = registry.begin(
        caller,
        &request,
        Resource::Model {
            kind: sailry_protocol::models::Kind::Embedding,
            id: "fixture-model".into(),
        },
    );
    let measured = Progress::Percent {
        percent: 25,
        bytes_per_second: Some(120),
    };
    report(Phase::Downloading, Some(measured));
    let first = tasks(&registry, caller).await.pop().unwrap();
    report(Phase::Downloading, Some(measured));
    report(
        Phase::Downloading,
        Some(Progress::Counts {
            completed: 0,
            total: 0,
            bytes_per_second: None,
        }),
    );
    assert_eq!(tasks(&registry, caller).await, vec![first.clone()]);
    report(Phase::Validating, None);
    assert_eq!(
        tasks(&registry, caller).await[0].revision,
        first.revision + 1
    );
}

#[tokio::test]
async fn reconnect_reads_retained_rows() {
    let registry = Arc::new(Registry::default());
    let caller = NodeId([1; 32]);
    let request = request(caller);
    registry.begin(
        caller,
        &request,
        Resource::Plugin {
            name: "fixture".into(),
        },
    );
    registry.finish(caller, request.id, &Err(Fault::invalid("fixture failure")));
    let original = tasks(&registry, caller).await;
    assert_eq!(tasks(&registry, caller).await, original);
    registry.dismiss(caller, request.id, None).unwrap();
    assert!(tasks(&registry, caller).await.is_empty());
}

#[tokio::test]
async fn shutdown_closes_subscription() {
    let registry = Registry::default();
    let stop = CancellationToken::new();
    let mut stream = registry.subscribe(NodeId([1; 32]), NodeId([1; 32]), stop.clone());
    stream.next().await.unwrap();
    stop.cancel();
    assert_eq!(
        stream.next().await.unwrap_err().code,
        ErrorCode::Unavailable
    );
}
