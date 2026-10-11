use super::*;

#[gpui::test]
fn resumes_paused_queue(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let first = fixture.enqueue("First queued message");
        fixture.enqueue("Second queued message");
        let (view, visual) = super::super::super::tests::fixture::open(
            cx,
            fixture.binding.clone(),
            fixture.session.clone(),
        );
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).queue.paused
                && panel.read(cx).queue.items.len() == 2
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Keep the unsent draft");
        assert!(visual.debug_bounds("queue-resume").is_none());
        click(visual, selector("send", first));
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).queue.items.is_empty()
                && view.read(cx).active().is_none()
        });
        assert!(!panel.read_with(visual, |panel, _| panel.queue.paused));
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Keep the unsent draft"
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
use sailry_link::{Admission, Pending as Future, Subscription};
use sailry_protocol::{Fault, RequestId, Topic};
use std::sync::Mutex;

#[gpui::test]
fn dismisses_removed_row_tooltips(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for visible in [false, true] {
            let fixture = Fixture::new(remote);
            let turn = fixture.enqueue("Queued input");
            let (view, visual) = super::super::super::tests::fixture::open(
                cx,
                fixture.binding.clone(),
                fixture.session.clone(),
            );
            let panel = view.read_with(visual, |view, _| view.queue.clone());
            wait(visual, |cx| {
                panel.read(cx).connected
                    && panel.read(cx).queue.items.len() == 1
                    && view.read(cx).configured()
            });
            let bounds = visual.debug_bounds(selector("edit", turn)).unwrap();
            visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
            visual.run_until_parked();
            if visible {
                super::super::super::tests::fixture::hover(visual, selector("edit", turn));
                assert!(visual.debug_bounds("managed-tooltip-popup").is_some());
            }
            let revision = panel.read_with(visual, |panel, _| panel.queue.items[0].revision);
            fixture.execute(Command::RemoveQueuedTurn {
                turn,
                expected_revision: revision,
            });
            wait(visual, |cx| panel.read(cx).queue.items.is_empty());
            for _ in 0..12 {
                visual
                    .executor()
                    .advance_clock(std::time::Duration::from_millis(100));
                visual.run_until_parked();
                visual.update(|window, cx| window.draw(cx).clear(cx));
            }
            assert!(visual.debug_bounds("managed-tooltip-popup").is_none());
            visual.update(|window, _| window.remove_window());
            drop(view);
            fixture.close();
        }
    }
}

struct Gate {
    inner: Arc<dyn Transport>,
    remove: bool,
    entered: CancellationToken,
    release: CancellationToken,
}

impl Transport for Gate {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn dispatch(&self, request: Request) -> Future<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let hold = if self.remove {
                matches!(request.command, Command::RemoveQueuedTurn { .. })
            } else {
                matches!(request.command, Command::SubmitTurn { .. })
            };
            if hold {
                self.entered.cancel();
                self.release.cancelled().await;
            }
            self.inner.dispatch(request).await
        })
    }

    fn subscribe(&self, topic: Topic) -> Future<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
}

#[gpui::test]
fn preserves_concurrent_edits(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let turn = fixture.enqueue("Original queued input");
        let gate = Arc::new(Gate {
            inner: fixture.transport.clone(),
            remove: true,
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(gate.clone()));
        let (view, visual) =
            super::super::super::tests::fixture::open(cx, binding, fixture.session.clone());
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).queue.items.len() == 1
                && view.read(cx).configured()
        });
        click(visual, selector("edit", turn));
        wait(visual, |_| gate.entered.is_cancelled());
        let revision = panel.read_with(visual, |panel, _| panel.queue.items[0].revision);
        fixture.execute(Command::EditQueuedTurn {
            turn,
            expected_revision: revision,
            message: "Changed by another controller".into(),
        });
        gate.release.cancel();
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).retry.is_none()
                && panel.read(cx).queue.items[0].preview == "Changed by another controller"
        });
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.error),
            Some("queue_changed")
        );
        assert!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().is_empty()
                && !view.queue_edit_pending.get())
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

struct Lost {
    inner: Arc<dyn Transport>,
    ids: Mutex<Vec<RequestId>>,
    release: tokio::sync::Semaphore,
}

impl Transport for Lost {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn dispatch(&self, request: Request) -> Future<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let lose = if matches!(request.command, Command::RemoveQueuedTurn { .. }) {
                let mut ids = self.ids.lock().unwrap();
                ids.push(request.id);
                ids.len() == 1
            } else {
                false
            };
            let admission = self.inner.dispatch(request).await?;
            if !lose {
                return Ok(admission);
            }
            admission.completion.await.unwrap().unwrap();
            self.release.acquire().await.unwrap().forget();
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "injected missing response",
            ))
        })
    }

    fn subscribe(&self, topic: Topic) -> Future<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
}

#[gpui::test]
fn retry_restores_once(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let turn = fixture.enqueue("Original queued input");
        let transport = Arc::new(Lost {
            inner: fixture.transport.clone(),
            ids: Mutex::new(Vec::new()),
            release: tokio::sync::Semaphore::new(0),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(transport.clone()));
        let (view, visual) =
            super::super::super::tests::fixture::open(cx, binding, fixture.session.clone());
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).queue.items.len() == 1
                && view.read(cx).configured()
        });
        click(visual, selector("edit", turn));
        wait(visual, |cx| panel.read(cx).queue.items.is_empty());
        assert!(panel.read_with(visual, |panel, _| panel.pending));
        visual.simulate_input("Must not overwrite captured input");
        assert!(view.read_with(visual, |view, cx| view.input.read(cx).value().is_empty()));
        transport.release.add_permits(1);
        wait(visual, |cx| panel.read(cx).error.is_some());
        click(visual, "queue-retry");
        wait(visual, |cx| {
            !panel.read(cx).pending && panel.read(cx).retry.is_none()
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Original queued input"
        );
        assert_eq!(transport.ids.lock().unwrap().len(), 2);
        {
            let ids = transport.ids.lock().unwrap();
            assert_eq!(ids[0], ids[1]);
        }
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn sends_during_execution(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let slow = fixture.runtime.block_on(support::Server::start(true));
        let selected = provider(&slow.endpoint, "slow-model");
        fixture.execute(Command::SaveProvider {
            provider: selected.clone(),
            expected_revision: 0,
            secret: None,
        });
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 1,
            config: SessionConfig {
                assistant: None,
                resource: None,
                provider: selected.id,
                model: selected.default_model,
                credential: None,
                effort: Effort::High,
                mode: sailry_protocol::WorkMode::Code,
                permission: sailry_protocol::Permission::Ask,
            },
        }) else {
            panic!("session expected")
        };
        let Output::QueuedTurn(active) = fixture.execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: 2,
            message: "running".into(),
        }) else {
            panic!("turn expected")
        };
        // Running admission precedes ADK's canonical user event and model request.
        fixture.runtime.block_on(slow.wait_count(1));
        let gate = Arc::new(Gate {
            inner: fixture.transport.clone(),
            remove: false,
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(gate.clone()));
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(binding, Some(session), window, cx));
            entity = Some(view.clone());
            Root::new(
                cx.new(|_| crate::conversation::live::tests::fixture::Harness(view)),
                window,
                cx,
            )
        });
        let view = entity.unwrap();
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            view.read(cx).connected()
                && view.read(cx).active() == Some(active.id)
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.configure(fixture.session.config.clone(), window, cx)
            })
        });
        wait(visual, |cx| {
            view.read(cx).session.as_ref().unwrap().revision == 3 && !view.read(cx).pending
        });
        for (index, text) in ["queued first", "send this now"].iter().enumerate() {
            click(visual, "live-chat-input");
            visual.simulate_input(text);
            visual.simulate_keystrokes("enter");
            if index == 0 {
                // The pending row appears in this frame, before Node admission.
                visual.update(|window, cx| window.draw(cx).clear(cx));
                assert!(visual.debug_bounds("queue-outgoing").is_some());
                assert!(panel.read_with(visual, |panel, _| panel.queue.items.is_empty()));
                wait(visual, |_| gate.entered.is_cancelled());
                assert!(view.read_with(visual, |view, _| view.pending));
                gate.release.cancel();
            }
            wait(visual, |cx| {
                panel.read(cx).queue.items.len() == index + 1 && !view.read(cx).pending
            });
        }
        // Changed settings queue new turns instead of guiding the running task.
        assert!(panel.read_with(visual, |panel, _| {
            panel.queue.items.iter().all(|item| item.guidance.is_none())
        }));
        assert!(visual.debug_bounds("queue-pause").is_none());
        fixture.execute(Command::SetQueuePaused {
            session: fixture.session.id,
            expected_revision: panel.read_with(visual, |panel, _| panel.queue.revision),
            paused: true,
        });
        wait(visual, |cx| {
            panel.read(cx).queue.paused && !panel.read(cx).pending
        });
        assert!(visual.debug_bounds("queue-pause").is_none());
        assert_eq!(
            view.read_with(visual, |view, _| view.active()),
            Some(active.id)
        );
        assert!(visual.debug_bounds("queue-resume").is_none());
        let (first, turn) = panel.read_with(visual, |panel, _| {
            (panel.queue.items[0].turn, panel.queue.items[1].turn)
        });
        click(visual, selector("send", turn));
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .filter(|run| run.status == Status::Completed)
                        .count()
                        == 2
                })
        });
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert!(
                page.runs
                    .iter()
                    .any(|run| run.turn == active.id && run.status == Status::Cancelled)
            );
            let executed: Vec<_> = page
                .entries
                .iter()
                .filter(|entry| entry.author == "user")
                .map(|entry| entry.turn)
                .collect();
            assert_eq!(executed, [active.id, turn, first]);
        });
        assert_eq!(slow.requests.lock().unwrap().len(), 1);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
