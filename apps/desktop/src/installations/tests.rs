use super::*;
use core::prelude::v1::test;
use sailry_protocol::installation::Task as Observed;

fn key() -> Key {
    (Location::Node(NodeId([1; 32])), RequestId::new())
}

fn entry() -> Entry {
    Entry {
        order: 1,
        resource: Resource::Plugin {
            name: "example".into(),
        },
        status: Presentation::Starting,
        retry: Some(Rc::new(|_, _, _| {})),
        finished: false,
        connected: true,
        dismissing: false,
    }
}

fn failure() -> Fault {
    Fault::new(ErrorCode::InvalidRequest, "invalid package")
}

struct DraftTransport;

impl sailry_link::Transport for DraftTransport {
    fn target(&self) -> NodeId {
        NodeId([1; 32])
    }

    fn dispatch(
        &self,
        _: Request,
    ) -> sailry_link::Pending<'_, Result<sailry_link::Admission, Fault>> {
        panic!("draft preparation must not dispatch")
    }

    fn subscribe(
        &self,
        _: sailry_protocol::Topic,
    ) -> sailry_link::Pending<'_, Result<Box<dyn sailry_link::Subscription>, Fault>> {
        panic!("draft preparation must not subscribe")
    }
}

#[test]
fn captures_plugin_installs() {
    let request = Request::new(
        NodeId([1; 32]),
        Command::InstallBundledPlugin {
            name: "example".into(),
            expected_revision: 1,
        },
    )
    .with_plugin(sailry_protocol::plugin::Context {
        invocation: None,
        turn: None,
        surface: sailry_protocol::plugin::desktop::Surface::Settings,
        package: sailry_protocol::plugin::Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 1,
        },
        worktree: None,
        session: None,
    });
    assert_eq!(
        resource(&request.command),
        Some(Resource::Plugin {
            name: "example".into(),
        })
    );
    assert!(resource(&Command::Snapshot).is_none());
    let client = Client::new(Arc::new(DraftTransport));
    let recovered = Attempt::Recover.request(&client, &request);
    assert_eq!(recovered, request);
    let fresh = Attempt::New.request(&client, &request);
    assert_ne!(fresh.id, request.id);
    assert_eq!(fresh.target, request.target);
    assert_eq!(fresh.command, request.command);
    assert_eq!(fresh.plugin, request.plugin);
}

fn observed(id: RequestId, state: Status) -> View {
    View {
        connected: true,
        error: None,
        tasks: vec![Observed {
            request: id,
            revision: 1,
            resource: Resource::Plugin {
                name: "example".into(),
            },
            state,
        }],
    }
}

#[gpui::test]
fn disappearance_does_not_complete_owned_requests(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let queue = registry(cx);
        let key = key();
        queue.update(cx, |queue, _| {
            queue.entries.insert(key, entry());
            queue.accept(
                key.0,
                View {
                    connected: true,
                    ..Default::default()
                },
            );
            assert!(!queue.entries[&key].finished);
        });
        Ticket {
            queue: queue.clone(),
            key,
        }
        .record(Ok(()), cx);
        assert!(queue.read(cx).entries.is_empty());
        assert!(!visible(cx));
    });
}

#[gpui::test]
fn actions_wait_for_original_completion(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let queue = registry(cx);
        let key = key();
        queue.update(cx, |queue, _| {
            queue.entries.insert(key, entry());
            queue.accept(key.0, observed(key.1, Status::Failed { error: failure() }));
            assert!(!queue.entries[&key].actionable());
        });
        Ticket {
            queue: queue.clone(),
            key,
        }
        .record(Err(failure()), cx);
        assert!(queue.read(cx).entries[&key].actionable());
        assert!(visible(cx));
    });
}

#[gpui::test]
fn confirmed_unavailability_remains_a_failure(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let queue = registry(cx);
        let raw = key();
        let confirmed = key();
        queue.update(cx, |queue, _| {
            queue.entries.insert(raw, entry());
            queue.entries.insert(confirmed, entry());
        });
        let error = Fault::new(ErrorCode::Unavailable, "model service is unavailable");
        Ticket {
            queue: queue.clone(),
            key: raw,
        }
        .record(Err(error.clone()), cx);
        Ticket {
            queue: queue.clone(),
            key: confirmed,
        }
        .record_outcome(Err(error), cx);
        assert!(matches!(
            queue.read(cx).entries[&raw].status,
            Presentation::Observed(Status::Uncertain { .. })
        ));
        assert!(matches!(
            queue.read(cx).entries[&confirmed].status,
            Presentation::Observed(Status::Failed { .. })
        ));
        assert!(queue.read(cx).entries[&confirmed].actionable());
    });
}

#[gpui::test]
fn recovers_unowned_terminal_tasks(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let queue = registry(cx);
        let key = key();
        queue.update(cx, |queue, _| {
            queue.accept(key.0, observed(key.1, Status::Failed { error: failure() }));
            let entry = &queue.entries[&key];
            assert!(entry.actionable());
            assert!(entry.retry.is_none());
        });
    });
}

#[gpui::test]
fn retains_uncertainty_after_unowned_disappearance(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let queue = registry(cx);
        let key = key();
        queue.update(cx, |queue, _| {
            queue.accept(key.0, observed(key.1, Status::Queued));
            queue.accept(
                key.0,
                View {
                    connected: true,
                    ..Default::default()
                },
            );
            assert!(matches!(
                queue.entries[&key].status,
                Presentation::Observed(Status::Uncertain { .. })
            ));
            assert!(queue.entries[&key].actionable());
        });
    });
}

#[gpui::test]
fn exposes_observed_model_progress(cx: &mut TestAppContext) {
    use sailry_protocol::installation::{Phase, Progress};
    cx.update(|cx| {
        let node = NodeId([1; 32]);
        assert_eq!(model_progress(node, "model", cx), None);
        let queue = registry(cx);
        let request = RequestId::new();
        let progress = Progress::Percent {
            percent: 40,
            bytes_per_second: Some(2048),
        };
        queue.update(cx, |queue, _| {
            queue.accept(
                Location::Node(node),
                View {
                    connected: true,
                    error: None,
                    tasks: vec![Observed {
                        request,
                        revision: 1,
                        resource: Resource::Model {
                            kind: sailry_protocol::models::Kind::Embedding,
                            id: "model".into(),
                        },
                        state: Status::Running {
                            phase: Phase::Downloading,
                            progress: Some(progress),
                        },
                    }],
                },
            );
        });
        assert_eq!(model_progress(node, "model", cx), Some(progress));
        assert_eq!(model_progress(node, "other", cx), None);
        assert_eq!(model_progress(NodeId([2; 32]), "model", cx), None);
    });
}
