use super::*;
use sailry_link::{Admission, Pending as Future, Subscription};
use sailry_protocol::{Fault, RequestId, Topic};
use std::sync::Mutex;

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
            let lose = if matches!(request.command, Command::EditQueuedTurn { .. }) {
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
fn retry_preserves_typing(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let turn = fixture.enqueue("original");
        let transport = Arc::new(Lost {
            inner: fixture.transport.clone(),
            ids: Mutex::new(Vec::new()),
            release: tokio::sync::Semaphore::new(0),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(transport.clone()));
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(binding, Some(fixture.session.clone()), window, cx));
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
            panel.read(cx).connected
                && panel.read(cx).queue.items.len() == 1
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        click(visual, "live-queue");
        click(visual, selector("edit", turn));
        wait(visual, |cx| panel.read(cx).editing.is_some());
        visual.update(|window, cx| {
            let panel = panel.read(cx);
            assert!(
                panel
                    .editing
                    .as_ref()
                    .unwrap()
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
        });
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("first edit");
        panel.read_with(visual, |panel, cx| {
            assert_eq!(
                panel.editing.as_ref().unwrap().input.read(cx).value(),
                "first edit"
            );
        });
        visual.simulate_keystrokes("enter");
        assert!(panel.read_with(visual, |panel, _| panel.pending));
        wait(visual, |cx| panel.read(cx).queue.items[0].revision == 2);
        assert!(panel.read_with(visual, |panel, _| panel.pending));
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("new typing while saving");
        visual.simulate_keystrokes("escape");
        transport.release.add_permits(1);
        wait(visual, |cx| panel.read(cx).error.is_some());
        click(visual, "live-queue");
        click(visual, "queue-retry");
        wait(visual, |cx| {
            !panel.read(cx).pending && panel.read(cx).retry.is_none()
        });
        panel.read_with(visual, |panel, cx| {
            let editing = panel.editing.as_ref().unwrap();
            assert_eq!(editing.input.read(cx).value(), "new typing while saving");
            assert_eq!(editing.revision, 2);
        });
        click(visual, "queue-edit-save");
        wait(visual, |cx| {
            panel.read(cx).editing.is_none() && panel.read(cx).queue.items[0].revision == 3
        });
        let ids = transport.ids.lock().unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
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
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(fixture.binding.clone(), Some(session), window, cx));
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
            wait(visual, |cx| {
                panel.read(cx).queue.items.len() == index + 1 && !view.read(cx).pending
            });
        }
        click(visual, "live-queue");
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
        let turn = panel.read_with(visual, |panel, _| panel.queue.items[1].turn);
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
            assert_eq!(executed[1], turn);
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
