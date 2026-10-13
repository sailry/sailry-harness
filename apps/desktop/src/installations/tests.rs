use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
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

fn settled_bounds(visual: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    visual.update(|window, cx| window.render_frame(cx));
    let started = std::time::Instant::now();
    let mut previous = None;
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.render_frame(cx));
        let bounds = visual.debug_bounds(selector).unwrap();
        let elapsed = started.elapsed();
        if elapsed >= *gpui_kit::component::dialog::ANIMATION_DURATION && previous == Some(bounds) {
            return bounds;
        }
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "installation layout deadline"
        );
        previous = Some(bounds);
        std::thread::sleep(std::time::Duration::from_millis(10));
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
                    entry.status = if order == 0 {
                        Presentation::Waiting
                    } else {
                        Presentation::Observed(Status::Running {
                            phase: sailry_protocol::installation::Phase::Downloading,
                            progress: (order == 1).then_some(
                                sailry_protocol::installation::Progress::Counts {
                                    completed: 3,
                                    total: 4,
                                    bytes_per_second: Some(1_048_576),
                                },
                            ),
                        })
                    };
                    entry.connected = order != 2;
                    queue.entries.insert(key, entry);
                    key.1
                })
                .collect::<Vec<_>>();
            queue.changed(cx);
            queue.open(window, cx);
            tasks
        })
    });
    for width in [1000., 380.] {
        visual.simulate_resize(size(px(width), px(800.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let list = visual.debug_bounds("installation-list").unwrap();
        let rem = visual.update(|window, _| window.rem_size());
        let mut previous = None;
        for request in &tasks {
            let mut bounds = |suffix| {
                visual
                    .debug_bounds(Box::leak(
                        format!("installation-{suffix}-{request}").into_boxed_str(),
                    ))
                    .unwrap()
            };
            let row = bounds("card");
            let title = bounds("title");
            let status = bounds("status");
            let progress = bounds("progress");
            assert_eq!(row.size.height, rem * 3.5);
            assert!(row.left() >= list.left() && row.right() <= list.right());
            assert!(title.left() >= row.left() && status.left() >= row.left());
            assert!(title.bottom() <= status.top());
            assert!(status.bottom() <= row.bottom());
            assert!(title.right() < progress.left() && status.right() < progress.left());
            assert!(progress.right() <= row.right());
            assert_eq!(progress.size.height, px(6.));
            assert!((progress.center().y - row.center().y).abs() <= px(1.));
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("installation-actions-{request}").into_boxed_str()
                    ))
                    .is_none()
            );
            if let Some(previous) = previous {
                assert!(previous <= row.top());
            }
            previous = Some(row.bottom());
        }
        visual.update(|window, cx| {
            window.render_frame(cx);
            let title = window.find_by_label(&"a".repeat(64));
            assert_eq!(title.role(), Some(Role::Label));
            let status = format!(
                "{} · {} · 3/4 · {}",
                tr("installation_downloading"),
                tr("installation_node"),
                rust_i18n::t!("installation_speed", value = "1.0"),
            );
            assert_eq!(window.find_by_label(&status).role(), Some(Role::Label));
            let offline = rust_i18n::t!(
                "installation_offline",
                phase = tr("installation_downloading"),
            );
            let status = format!("{offline} · {}", tr("installation_node"));
            assert_eq!(window.find_by_label(&status).role(), Some(Role::Label));
        });
    }
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn icon_actions_preserve_completion_gates(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let frame = cx.new(|_| Frame);
        Root::new(frame, window, cx)
    });
    visual.update(|window, _| window.activate_window());
    let attempts = Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = key();
    let queue = visual.update(|window, cx| {
        let queue = registry(cx);
        queue.update(cx, |queue, cx| {
            let mut entry = entry();
            entry.resource = Resource::Plugin {
                name: "Preserve the complete captured installation title".repeat(5),
            };
            let attempts = attempts.clone();
            entry.retry = Some(Rc::new(move |attempt, _, _| {
                attempts.borrow_mut().push(attempt);
            }));
            queue.entries.insert(captured, entry);
            queue.changed(cx);
            queue.open(window, cx);
        });
        queue
    });
    let selector =
        |action| Box::leak(format!("installation-{action}-{}", captured.1).into_boxed_str());
    for width in [1000., 380.] {
        visual.simulate_resize(size(px(width), px(800.)));
        for (uncertain, finished, retryable, dismissing) in [
            (false, false, true, false),
            (false, true, true, true),
            (false, true, false, false),
            (false, true, true, false),
            (true, true, true, false),
        ] {
            queue.update(visual, |queue, cx| {
                let entry = queue.entries.get_mut(&captured).unwrap();
                entry.status = Presentation::Observed(if uncertain {
                    Status::Uncertain { error: failure() }
                } else {
                    Status::Failed { error: failure() }
                });
                entry.finished = finished;
                entry.dismissing = dismissing;
                let attempts = attempts.clone();
                entry.retry = retryable.then(|| {
                    Rc::new(move |attempt, _: &mut Window, _: &mut App| {
                        attempts.borrow_mut().push(attempt);
                    }) as Retry
                });
                queue.changed(cx);
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds(selector("progress")).is_none());
            let actionable = finished && !dismissing;
            assert_eq!(
                visual.debug_bounds(selector("remove")).is_some(),
                actionable
            );
            assert_eq!(
                visual.debug_bounds(selector("retry")).is_some(),
                actionable && retryable
            );
            if !actionable {
                continue;
            }
            let row = settled_bounds(visual, selector("card"));
            let title = visual.debug_bounds(selector("title")).unwrap();
            let status = visual.debug_bounds(selector("status")).unwrap();
            let remove = visual.debug_bounds(selector("remove")).unwrap();
            assert!(title.right() < remove.left() && status.right() < remove.left());
            assert!(remove.right() <= row.right());
            assert!((remove.center().y - row.center().y).abs() <= px(1.));
            visual.simulate_mouse_move(row.center(), None, Modifiers::default());
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(visual.debug_bounds(selector("card")), Some(row));
            visual.update(|window, cx| {
                window.render_frame(cx);
                assert_eq!(
                    window.find_by_label(&tr("installation_remove")).role(),
                    Some(Role::Button)
                );
            });
            if retryable {
                let retry = visual.debug_bounds(selector("retry")).unwrap();
                assert!(retry.right() <= remove.left());
                assert!((retry.center().y - row.center().y).abs() <= px(1.));
                visual.simulate_click(retry.center(), Modifiers::default());
                visual.run_until_parked();
                let count = attempts.borrow().len();
                assert!(count > 0);
                assert_eq!(
                    matches!(attempts.borrow()[count - 1], Attempt::Recover),
                    uncertain
                );
                visual.update(|window, cx| {
                    window.focus_next(cx);
                    window.render_frame(cx);
                    for _ in 0..8 {
                        window.press("tab", cx);
                        if window.find_by_label(&tr("installation_retry")).focused() == Some(true) {
                            break;
                        }
                    }
                    assert_eq!(
                        window.find_by_label(&tr("installation_retry")).focused(),
                        Some(true),
                    );
                    window.press("space", cx);
                });
                visual.run_until_parked();
                assert_eq!(attempts.borrow().len(), count + 1);
                assert_eq!(
                    matches!(attempts.borrow()[count], Attempt::Recover),
                    uncertain
                );
            }
        }
    }
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn remove_icon_dismisses_without_replaying(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for remote in [false, true] {
        let fixture = crate::plugins::fixture::Fixture::new(remote);
        let client = fixture.binding.client.clone();
        let request = client.prepare(Command::InstallBundledPlugin {
            name: "missing-installation-fixture".into(),
            expected_revision: 0,
        });
        let error = fixture
            .runtime
            .block_on(client.execute(request.clone()))
            .unwrap_err();
        let captured = (Location::Node(client.target()), request.id);
        let (_, visual) = cx.add_window_view(|window, cx| {
            let frame = cx.new(|_| Frame);
            Root::new(frame, window, cx)
        });
        let queue = visual.update(|window, cx| {
            let queue = registry(cx);
            queue.update(cx, |queue, cx| {
                queue.bind(client.clone(), fixture.runtime.clone(), cx);
                queue.entries.insert(captured, entry());
                queue.changed(cx);
                queue.open(window, cx);
            });
            queue
        });
        visual.update(|_, cx| {
            Ticket {
                queue: queue.clone(),
                key: captured,
            }
            .record(Err(error.clone()), cx);
        });
        visual.simulate_resize(size(px(380.), px(800.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let selector = Box::leak(format!("installation-remove-{}", request.id).into_boxed_str());
        let remove = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(remove.center(), Modifiers::default());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            if !queue.read_with(visual, |queue, _| queue.entries.contains_key(&captured)) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "installation dismissal deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let output = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ReadInstallations {
                requests: vec![request.id],
            })))
            .unwrap();
        assert_eq!(output, Output::Installations(Vec::new()));
        assert_eq!(
            fixture.runtime.block_on(client.outcome(&request)).unwrap(),
            sailry_protocol::RequestOutcome::Completed(Box::new(Err(error))),
        );
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        visual.update(|window, _| window.remove_window());
        fixture.close();
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
    visual.update(|window, _| window.activate_window());
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
    visual.update(|window, _| window.activate_window());
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
