use super::*;
use sailry_link::{Admission, Pending as Future, Subscription};
use sailry_protocol::{Fault, RequestId, Topic};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

struct HeldAction {
    inner: Arc<dyn Transport>,
    guide: bool,
    release: CancellationToken,
    fail: AtomicBool,
    requests: Mutex<Vec<RequestId>>,
}

impl Transport for HeldAction {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn subscribe(&self, topic: Topic) -> Future<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Future<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let held = if self.guide {
                matches!(request.command, Command::GuideQueuedTurn { .. })
            } else {
                matches!(request.command, Command::SubmitTurn { .. })
            };
            if held {
                self.requests.lock().unwrap().push(request.id);
                self.release.cancelled().await;
                if self.fail.swap(false, Ordering::SeqCst) {
                    return Err(Fault::new(
                        if self.guide {
                            ErrorCode::OutcomeUnknown
                        } else {
                            ErrorCode::Unavailable
                        },
                        "injected missing response",
                    ));
                }
            }
            self.inner.dispatch(request).await
        })
    }
}

/// Starts a task against `server`; tool-capable models can wait for a user answer.
fn running(
    fixture: &mut Fixture,
    server: support::Server,
    tools: bool,
) -> (support::Server, sailry_protocol::QueuedTurn) {
    let mut provider = provider(&server.endpoint, "guide-model");
    provider.models[0].tools = tools;
    fixture.execute(Command::SaveProvider {
        provider: provider.clone(),
        expected_revision: 0,
        secret: None,
    });
    let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        config: SessionConfig {
            provider: provider.id,
            model: provider.default_model,
            ..fixture.session.config.clone()
        },
    }) else {
        panic!("session expected")
    };
    fixture.session = session;
    let Output::QueuedTurn(active) = fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        message: "Initial task".into(),
    }) else {
        panic!("turn expected")
    };
    fixture.runtime.block_on(server.wait_count(1));
    (server, active)
}

fn status(view: &Entity<View>, turn: TurnId, cx: &App) -> Option<Status> {
    view.read(cx)
        .history
        .snapshot
        .as_ref()?
        .page
        .runs
        .iter()
        .find(|run| run.turn == turn)
        .map(|run| run.status)
}

fn users(view: &Entity<View>, cx: &App) -> Vec<TurnId> {
    view.read(cx)
        .history
        .snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .page
                .entries
                .iter()
                .filter(|entry| entry.author == "user")
                .map(|entry| entry.turn)
                .collect()
        })
        .unwrap_or_default()
}

#[gpui::test]
fn preserves_unresolved_submission(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let server = fixture.runtime.block_on(support::Server::tools(vec![(
            "ask_user".into(),
            serde_json::json!({"prompt":"Describe the task","input":{"kind":"text","multiline":false,"max_bytes":256}}),
        )]));
        let (_, active) = running(&mut fixture, server, true);
        let selected = fixture.enqueue("Earlier queued guidance");
        let transport = Arc::new(HeldAction {
            inner: fixture.transport.clone(),
            guide: false,
            release: CancellationToken::new(),
            fail: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(transport.clone()));
        let (view, visual) =
            super::super::super::tests::fixture::open(cx, binding, fixture.session.clone());
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).recipient == Some(active.id)
                && panel.read(cx).queue.items.len() == 1
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        visual.update(|window, cx| {
            use gpui_kit::test::TestWindowExt;
            window.render_frame(cx);
            assert_eq!(
                window
                    .find(format!("live-queue-guide-{selected}"))
                    .focused(),
                Some(false)
            );
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Ordinary pending submission");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx).pending && panel.read(cx).guidance_blocked
        });
        let request = view.read_with(visual, |view, _| {
            view.outgoing.as_ref().unwrap().request.unwrap()
        });
        for pending in [true, false] {
            assert_eq!(view.read_with(visual, |view, _| view.pending), pending);
            visual.update(|window, cx| {
                use gpui_kit::test::TestWindowExt;
                window.render_frame(cx);
                let control = window.find(format!("live-queue-guide-{selected}"));
                assert_eq!(control.role(), Some(Role::Button));
                assert!(control.visible());
                // GPUI lacks aria-disabled; Kit removes the focus binding and
                // activation handler instead. Exercise the real disabled control.
                assert_eq!(control.focused(), None);
                window.click(format!("live-queue-guide-{selected}"), cx);
                // The owner guard also handles a click captured before controls became disabled.
                panel.update(cx, |panel, cx| {
                    let revision = panel
                        .queue
                        .items
                        .iter()
                        .find(|item| item.turn == selected)
                        .unwrap()
                        .revision;
                    panel.execute(
                        Command::GuideQueuedTurn {
                            turn: selected,
                            recipient: active.id,
                            expected_revision: revision,
                        },
                        window,
                        cx,
                    );
                });
            });
            assert!(panel.read_with(visual, |panel, _| !panel.pending && panel.retry.is_none()));
            view.read_with(visual, |view, _| {
                let message = view.outgoing.as_ref().unwrap();
                assert_eq!(message.request, Some(request));
                assert_eq!(message.input.text, "Ordinary pending submission");
                assert!(message.queued);
            });
            if pending {
                transport.release.cancel();
                wait(visual, |cx| {
                    !view.read(cx).pending && view.read(cx).retry.is_some()
                });
            }
        }
        visual.update(|window, cx| view.update(cx, |view, cx| view.retry(window, cx)));
        wait(visual, |cx| {
            !view.read(cx).pending
                && view.read(cx).retry.is_none()
                && view.read(cx).outgoing.is_none()
                && panel.read(cx).queue.items.len() == 2
        });
        let requests = transport.requests.lock().unwrap();
        assert_eq!(*requests, [request, request]);
        drop(requests);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            ""
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn preserves_draft_during_guidance(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let server = fixture.runtime.block_on(support::Server::tools(vec![(
            "ask_user".into(),
            serde_json::json!({"prompt":"Describe the task","input":{"kind":"text","multiline":false,"max_bytes":256}}),
        )]));
        let (_, active) = running(&mut fixture, server, true);
        let selected = fixture.enqueue("Earlier queued guidance");
        let transport = Arc::new(HeldAction {
            inner: fixture.transport.clone(),
            guide: true,
            release: CancellationToken::new(),
            fail: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(transport.clone()));
        let (view, visual) =
            super::super::super::tests::fixture::open(cx, binding, fixture.session.clone());
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).recipient == Some(active.id)
                && panel.read(cx).queue.items.len() == 1
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Keep this new draft");
        click(visual, selector("guide", selected));
        wait(visual, |cx| {
            panel.read(cx).pending && view.read(cx).guidance_pending.get()
        });
        let request = view.read_with(visual, |view, _| {
            view.outgoing.as_ref().unwrap().request.unwrap()
        });
        for pending in [true, false] {
            assert_eq!(panel.read_with(visual, |panel, _| panel.pending), pending);
            visual.simulate_keystrokes("enter");
            visual.update(|window, cx| {
                view.update(cx, |view, cx| {
                    // A stale click and an HTML form answer use the same busy boundary.
                    view.send(window, cx);
                    let (sender, _) = tokio::sync::watch::channel(None);
                    assert!(!view.send_document(
                        fixture.session.id,
                        "Form answer".into(),
                        sender,
                        window,
                        cx
                    ));
                })
            });
            view.read_with(visual, |view, cx| {
                assert!(view.busy());
                assert_eq!(view.input.read(cx).value(), "Keep this new draft");
                let outgoing = view.outgoing.as_ref().unwrap();
                assert_eq!(outgoing.request, Some(request));
                assert_eq!(outgoing.turn, Some(selected));
                assert!(!outgoing.queued);
            });
            assert_eq!(
                panel.read_with(visual, |panel, _| panel.queue.items.len()),
                1
            );
            if pending {
                transport.release.cancel();
                wait(visual, |cx| {
                    !panel.read(cx).pending && panel.read(cx).retry.is_some()
                });
            }
        }
        visual.update(|window, cx| panel.update(cx, |panel, cx| panel.retry(window, cx)));
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).retry.is_none()
                && !view.read(cx).busy()
                && panel.read(cx).queue.items[0].guidance == Some(active.id)
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Keep this new draft"
        );
        let requests = transport.requests.lock().unwrap();
        assert_eq!(*requests, [request, request]);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn clears_deselected_guidance(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let server = fixture.runtime.block_on(support::Server::tools(vec![(
            "ask_user".into(),
            serde_json::json!({"prompt":"Describe the task","input":{"kind":"text","multiline":false,"max_bytes":256}}),
        )]));
        let (_, active) = running(&mut fixture, server, true);
        let (view, visual) = super::super::super::tests::fixture::open(
            cx,
            fixture.binding.clone(),
            fixture.session.clone(),
        );
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).recipient == Some(active.id)
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
                && view
                    .read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| !snapshot.page.questions.is_empty())
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Keep this composer draft");
        for action in ["remove", "edit", "stop"] {
            let selected = fixture.enqueue(format!("Guidance to {action}"));
            wait(visual, |cx| {
                panel
                    .read(cx)
                    .queue
                    .items
                    .iter()
                    .any(|item| item.turn == selected)
            });
            click(visual, selector("guide", selected));
            wait(visual, |cx| {
                !panel.read(cx).pending
                    && view.read(cx).outgoing.as_ref().is_some_and(|message| {
                        message.turn == Some(selected)
                            && message
                                .guidance
                                .as_ref()
                                .is_some_and(|guidance| guidance.revision.is_some())
                    })
            });
            let revision = panel.read_with(visual, |panel, _| {
                panel
                    .queue
                    .items
                    .iter()
                    .find(|item| item.turn == selected)
                    .unwrap()
                    .revision
            });
            fixture.execute(match action {
                "remove" => Command::RemoveQueuedTurn {
                    turn: selected,
                    expected_revision: revision,
                },
                "edit" => Command::EditQueuedTurn {
                    turn: selected,
                    expected_revision: revision,
                    message: "Edited pending input".into(),
                },
                "stop" => Command::StopTurn { turn: active.id },
                _ => unreachable!(),
            });
            wait(visual, |cx| view.read(cx).outgoing.is_none());
            assert!(!view.read_with(visual, |view, _| view.rows.contains(&selected)));
            assert_eq!(
                view.read_with(visual, |view, cx| view.input.read(cx).value()),
                "Keep this composer draft"
            );
        }
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn guide_interrupts_response(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let release = CancellationToken::new();
        let server = fixture.runtime.block_on(support::Server::markdown_held(
            "Natural answer".into(),
            release.clone(),
        ));
        let (server, active) = running(&mut fixture, server, false);
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config: SessionConfig {
                permission: sailry_protocol::Permission::Full,
                ..fixture.session.config.clone()
            },
        }) else {
            panic!("session expected")
        };
        fixture.session = session;
        let selected = fixture.enqueue("Use this guidance 中文 🙂");
        let (view, visual) = super::super::super::tests::fixture::open(
            cx,
            fixture.binding.clone(),
            fixture.session.clone(),
        );
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).recipient == Some(active.id)
                && panel.read(cx).queue.items.len() == 1
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Keep this composer draft");
        assert!(visual.debug_bounds(selector("send", selected)).is_some());
        let guide = visual.debug_bounds(selector("guide", selected)).unwrap();
        assert!(guide.size.width <= px(32.));
        click(visual, selector("guide", selected));
        // The awaited response yields at once; guidance replaces the selected input.
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).queue.items.is_empty()
                && status(&view, active.id, cx) == Some(Status::Continued)
        });
        fixture.runtime.block_on(server.wait_count(2));
        wait(visual, |cx| users(&view, cx).len() == 2);
        let admitted = visual.update(|_, cx| users(&view, cx)[1]);
        assert_ne!(admitted, selected);
        assert_eq!(
            view.read_with(visual, |view, _| view.rows.clone()),
            [active.id, admitted]
        );
        assert!(view.read_with(visual, |view, _| view.outgoing.is_none()));
        release.cancel();
        wait(visual, |cx| {
            let users = users(&view, cx);
            users.len() == 2 && status(&view, users[1], cx) == Some(Status::Completed)
        });
        let guided = visual.update(|_, cx| users(&view, cx));
        assert_eq!(guided[0], active.id);
        assert_ne!(guided[1], selected);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Keep this composer draft"
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn enter_queues_until_guided(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let server = fixture.runtime.block_on(support::Server::tools(vec![(
            "ask_user".into(),
            serde_json::json!({
                "prompt": "Describe the task",
                "input": {"kind": "text", "multiline": false, "max_bytes": 256}
            }),
        )]));
        let (server, active) = running(&mut fixture, server, true);
        let (view, visual) = super::super::super::tests::fixture::open(
            cx,
            fixture.binding.clone(),
            fixture.session.clone(),
        );
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        // A pending answer keeps the task outside its interruptible model phase.
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).recipient == Some(active.id)
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
                && view
                    .read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| !snapshot.page.questions.is_empty())
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Prefer the smaller fix");
        visual.simulate_keystrokes("enter");
        // Enter only queues; the running task continues untouched.
        wait(visual, |cx| {
            !view.read(cx).pending
                && view.read(cx).outgoing.is_none()
                && panel.read(cx).queue.items.len() == 1
        });
        let (guided, guidance) = panel.read_with(visual, |panel, _| {
            (panel.queue.items[0].turn, panel.queue.items[0].guidance)
        });
        assert_eq!(guidance, None);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(selector("guided", guided)).is_none());
        assert!(visual.debug_bounds("live-outgoing-message").is_none());
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            ""
        );
        assert_eq!(
            visual.update(|_, cx| status(&view, active.id, cx)),
            Some(Status::Running)
        );
        click(visual, selector("guide", guided));
        assert!(view.read_with(visual, |view, _| {
            view.outgoing
                .as_ref()
                .is_some_and(|message| message.turn == Some(guided) && !message.queued)
        }));
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel
                    .read(cx)
                    .queue
                    .items
                    .first()
                    .is_some_and(|item| item.guidance == Some(active.id))
        });
        assert_eq!(visual.update(|_, cx| users(&view, cx)), [active.id]);
        assert_eq!(
            view.read_with(visual, |view, _| view.rows.last().copied()),
            Some(guided)
        );
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("live-outgoing-message").is_some());
        assert!(visual.debug_bounds(selector("guided", guided)).is_some());
        assert!(visual.debug_bounds(selector("guide", guided)).is_none());
        let question = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.questions[0].id
        });
        fixture.execute(Command::ResolveQuestion {
            session: fixture.session.id,
            question,
            response: sailry_protocol::conversation::question::Response::Answer(
                sailry_protocol::conversation::question::Answer::Text("Small".into()),
            ),
        });
        // The answered tool settles; guidance hands off before the next model request.
        wait(visual, |cx| {
            users(&view, cx) == [active.id, guided]
                && status(&view, active.id, cx) == Some(Status::Continued)
                && status(&view, guided, cx) == Some(Status::Completed)
        });
        assert!(view.read_with(visual, |view, _| view.outgoing.is_none()));
        assert_eq!(
            view.read_with(visual, |view, _| view.rows.clone()),
            [active.id, guided]
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
