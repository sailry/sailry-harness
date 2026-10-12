use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt as _};
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
fn identifies_captured_language_installs() {
    let request = Request::new(
        NodeId([1; 32]),
        Command::InstallCodeServer {
            install: sailry_protocol::code::Install {
                package: "analyzer".into(),
                version: "1".into(),
                purl: "pkg:generic/analyzer@1".into(),
                urls: vec![],
                licenses: vec![],
                command: None,
                revision: "a".repeat(64),
            },
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
        Some(Resource::Language {
            package: "analyzer".into(),
            version: "1".into()
        })
    );
    assert!(
        resource(&Command::UninstallCodeServer {
            package: "analyzer".into(),
            version: "1".into()
        })
        .is_none()
    );
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

struct Frame;
impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

#[gpui::test]
fn frames_each_resource_task(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let frame = cx.new(|_| Frame);
        Root::new(frame, window, cx)
    });
    let tasks = visual.update(|window, cx| {
        let queue = registry(cx);
        queue.update(cx, |queue, cx| {
            let resources = [
                Resource::Plugin {
                    name: "example".into(),
                },
                Resource::Model {
                    kind: sailry_protocol::models::Kind::Ocr,
                    id: "a".repeat(64),
                },
                Resource::Language {
                    package: "analyzer".into(),
                    version: "1".into(),
                },
            ];
            let tasks = resources
                .into_iter()
                .enumerate()
                .map(|(order, resource)| {
                    let key = key();
                    let mut entry = entry();
                    entry.order = order as u64;
                    entry.resource = resource;
                    queue.entries.insert(key, entry);
                    key.1
                })
                .collect::<Vec<_>>();
            queue.changed(cx);
            queue.open(window, cx);
            tasks
        })
    });
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let mut previous = None;
    for request in tasks {
        let card = Box::leak(format!("installation-card-{request}").into_boxed_str());
        let content = Box::leak(format!("installation-{request}").into_boxed_str());
        let card = visual.debug_bounds(card).unwrap();
        let content = visual.debug_bounds(content).unwrap();
        assert!(content.left() >= card.left() && content.right() <= card.right());
        assert!(content.top() >= card.top() && content.bottom() <= card.bottom());
        if let Some(previous) = previous {
            assert!(previous <= card.top());
        }
        previous = Some(card.bottom());
    }
}

#[gpui::test]
fn hide_reopens_the_same_list(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let frame = cx.new(|_| Frame);
        Root::new(frame, window, cx)
    });
    visual.update(|window, cx| {
        let queue = registry(cx);
        queue.update(cx, |queue, cx| {
            queue.entries.insert(key(), entry());
            queue.changed(cx);
            queue.open(window, cx);
            let list = queue.list.as_ref().unwrap().entity_id();
            queue.hide(window, cx);
            assert!(!window.has_active_dialog(cx));
            assert_eq!(queue.entries.len(), 1);
            queue.open(window, cx);
            assert_eq!(queue.list.as_ref().unwrap().entity_id(), list);
        });
    });
}

#[gpui::test]
fn completion_preserves_a_stacked_dialog(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let frame = cx.new(|_| Frame);
        Root::new(frame, window, cx)
    });
    let queue = visual.update(|window, cx| {
        let queue = registry(cx);
        queue.update(cx, |queue, cx| {
            queue.entries.insert(key(), entry());
            queue.changed(cx);
            queue.open(window, cx);
        });
        window.open_dialog(cx, |dialog, _, _| dialog.title("Confirmation"));
        let confirmation = window.focused(cx).unwrap();
        queue.update(cx, |queue, cx| {
            queue.entries.clear();
            queue.changed(cx);
            queue.close_empty(window, cx);
            assert!(confirmation.contains_focused(window, cx));
            assert!(window.has_active_dialog(cx));
        });
        window.close_dialog(cx);
        queue
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert!(queue.read(cx).window.is_none());
        assert!(!window.has_active_dialog(cx));
    });
}

struct Form {
    dialog: crate::ui::overlays::Owned,
}

#[gpui::test]
fn form_completion_preserves_the_task_dialog(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let frame = cx.new(|_| Frame);
        Root::new(frame, window, cx)
    });
    let form = visual.update(|window, cx| {
        let form = cx.new(|_| Form {
            dialog: Default::default(),
        });
        window.open_dialog(cx, |dialog, _, _| dialog.title("Install"));
        form.update(cx, |form, cx| {
            crate::ui::overlays::Owned::bind(form, |form| &mut form.dialog, window, cx)
        });
        let queue = registry(cx);
        queue.update(cx, |queue, cx| {
            queue.entries.insert(key(), entry());
            queue.changed(cx);
            queue.open(window, cx);
        });
        let progress = window.focused(cx).unwrap();
        form.update(cx, |form, cx| form.dialog.close(window, cx));
        assert!(progress.contains_focused(window, cx));
        assert!(window.has_active_dialog(cx));
        queue.update(cx, |queue, cx| queue.hide(window, cx));
        form
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert!(!form.read(cx).dialog.presented(cx));
        assert!(!window.has_active_dialog(cx));
        assert!(visible(cx));
    });
}
