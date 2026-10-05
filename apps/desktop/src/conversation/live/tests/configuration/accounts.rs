use super::*;
use crate::conversation::live::tests::fixture::{init, open_session};
use sailry_link::{Admission, Pending as Future, Subscription};
use sailry_protocol::{ErrorCode, Fault, RequestId, Topic};
use std::sync::Mutex;

mod fixture;
use fixture::Fixture;

#[gpui::test]
fn selects_discovered_models(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::empty(remote);
        let (view, visual) = open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| {
            view.read(cx).configured()
                && view.read(cx).connected()
                && view.read(cx).defaults.connected
        });
        assert_eq!(fixture.provider.models.len(), 2);
        view.read_with(visual, |view, _| {
            assert_eq!(
                view.model_sources()
                    .find(|(node, _)| *node == fixture.target.id())
                    .unwrap()
                    .1
                    .models
                    .len(),
                2
            );
        });
        assert_eq!(
            view.read_with(visual, |view, _| view
                .config
                .as_ref()
                .unwrap()
                .model
                .clone()),
            "fixture"
        );
        let channel = view.read_with(visual, |view, _| {
            view.provider_ids[&(fixture.target.id(), fixture.provider.id)]
        });
        open_models(visual);
        let option = match channel {
            0 => "composer-model-option-0-unknown",
            1 => "composer-model-option-1-unknown",
            _ => panic!("unexpected account channel"),
        };
        click(visual, option);
        assert_eq!(
            view.read_with(visual, |view, _| view
                .config
                .as_ref()
                .unwrap()
                .model
                .clone()),
            "unknown"
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.config_owner),
            fixture.target.id()
        );
        assert!(
            fixture
                .target_server
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| !matches!(
                    request.path.as_str(),
                    "/responses" | "/chat/completions"
                ))
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture
            .runtime
            .block_on(fixture.source.take().unwrap().shutdown())
            .unwrap();
        fixture.runtime.block_on(fixture.target.shutdown()).unwrap();
        fixture
            .runtime
            .block_on(fixture.controller.close())
            .unwrap();
    }
}

fn choose(visual: &mut VisualTestContext, view: &Entity<View>, node: NodeId, provider: ProviderId) {
    let option = view.read_with(visual, |view, _| {
        match view.provider_ids[&(node, provider)] {
            0 => "composer-model-option-0-fixture",
            1 => "composer-model-option-1-fixture",
            _ => panic!("unexpected account channel"),
        }
    });
    open_models(visual);
    click(visual, option);
    assert_eq!(view.read_with(visual, |view, _| view.config_owner), node);
    assert!(visual.debug_bounds("model-controls").is_some());
    visual.simulate_keystrokes("escape");
}

fn completed(visual: &mut VisualTestContext, view: &Entity<View>, count: usize) {
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
                    == count
            })
    });
}

#[gpui::test]
fn selects_execution_account(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let (view, visual) = open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| {
            view.read(cx).node.connected && view.read(cx).defaults.connected
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config_owner),
            fixture.target.id()
        );
        assert_eq!(view.read_with(visual, |view, _| view.provider_ids.len()), 2);
        click(visual, "live-chat-input");
        visual.simulate_input("Use the execution account 🙂");
        // Importing an account grant is explicit and may be unavailable.
        choose(
            visual,
            &view,
            fixture.binding.defaults.target(),
            fixture.provider.id,
        );
        click(visual, "live-chat-input");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx).error.is_some() && !view.read(cx).pending
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(view.error, Some("chat_config_unavailable"));
            assert!(view.session.is_none() && view.retry.is_none());
            assert_eq!(view.input.read(cx).value(), "Use the execution account 🙂");
        });
        choose(visual, &view, fixture.target.id(), fixture.provider.id);
        assert_eq!(view.read_with(visual, |view, _| view.error), None);
        assert_eq!(
            view.read_with(visual, |view, _| view.settings_node()),
            fixture.target.id()
        );
        choose(
            visual,
            &view,
            fixture.binding.defaults.target(),
            fixture.provider.id,
        );
        assert_eq!(
            view.read_with(visual, |view, _| view
                .config
                .as_ref()
                .unwrap()
                .credential
                .clone()),
            fixture.source_provider.credential
        );
        choose(visual, &view, fixture.target.id(), fixture.provider.id);
        fixture
            .runtime
            .block_on(fixture.source.take().unwrap().shutdown())
            .unwrap();
        wait(visual, |cx| !view.read(cx).defaults.connected);
        assert!(view.read_with(visual, |view, _| view.connected()));
        click(visual, "live-chat-input");
        visual.simulate_keystrokes("enter");
        completed(visual, &view, 1);
        let session = view.read_with(visual, |view, cx| {
            assert!(view.input.read(cx).value().is_empty());
            assert!(view._defaults.is_none());
            view.session.clone().unwrap()
        });
        fixture.assert_execution(&session, 1);
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = open_session(cx, fixture.binding.clone(), Some(session.clone()));
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(
            view.read_with(visual, |view, _| view.config.clone().unwrap()),
            session.config
        );
        click(visual, "live-chat-input");
        visual.simulate_input("Resume the host session");
        visual.simulate_keystrokes("enter");
        completed(visual, &view, 2);
        fixture.assert_execution(&session, 2);
        visual.update(|window, _| window.remove_window());
        fixture.runtime.block_on(fixture.target.shutdown()).unwrap();
        fixture
            .runtime
            .block_on(fixture.controller.close())
            .unwrap();
    }
}

struct Lost {
    inner: Arc<dyn Transport>,
    ids: Mutex<Vec<RequestId>>,
    release: tokio::sync::Semaphore,
}

#[gpui::test]
fn retargets_draft_configuration(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let (view, visual) = open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| {
            view.read(cx).node.connected && view.read(cx).defaults.connected
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Preserve my draft");
        choose(
            visual,
            &view,
            fixture.binding.defaults.target(),
            fixture.provider.id,
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let mut binding = fixture.binding.clone();
                binding.client = binding.defaults.clone();
                binding.project = None;
                binding.worktree = None;
                view.retarget(binding, window, cx);
                view.retarget(fixture.binding.clone(), window, cx);
            });
        });
        wait(visual, |cx| {
            view.read(cx).configured() && view.read(cx).connected()
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(view.config_owner, fixture.target.id());
            assert_eq!(
                view.config.as_ref().unwrap().credential,
                fixture.provider.credential
            );
            assert_eq!(view.input.read(cx).value(), "Preserve my draft");
        });
        visual.update(|window, _| window.remove_window());
        let (fresh, visual) = open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| fresh.read(cx).configured());
        assert_eq!(
            fresh.read_with(visual, |view, _| view.config_owner),
            fixture.target.id()
        );
        visual.update(|window, _| window.remove_window());
        fixture
            .runtime
            .block_on(fixture.source.take().unwrap().shutdown())
            .unwrap();
        fixture.runtime.block_on(fixture.target.shutdown()).unwrap();
        fixture
            .runtime
            .block_on(fixture.controller.close())
            .unwrap();
    }
}

#[gpui::test]
fn selects_without_local_config(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let origin = &fixture.binding.defaults;
        fixture
            .runtime
            .block_on(origin.execute(origin.prepare(Command::RemoveProvider {
                provider: fixture.source_provider.id,
                expected_revision: fixture.source_provider.revision,
            })))
            .unwrap();
        fixture
            .runtime
            .block_on(fixture.source.take().unwrap().shutdown())
            .unwrap();
        let (view, visual) = open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| view.read(cx).node.connected);
        view.read_with(visual, |view, _| {
            assert!(!view.defaults.connected);
            assert_eq!(view.config_owner, fixture.target.id());
            assert!(view.configured());
            assert!(view.connected());
        });
        assert!(view.read_with(visual, |view, _| view.connected() && view.configured()));
        click(visual, "live-chat-input");
        visual.simulate_input("Use the available host account");
        visual.simulate_keystrokes("enter");
        completed(visual, &view, 1);
        view.read_with(visual, |view, _| {
            fixture.assert_execution(view.session.as_ref().unwrap(), 1)
        });
        visual.update(|window, _| window.remove_window());
        fixture.runtime.block_on(fixture.target.shutdown()).unwrap();
        fixture
            .runtime
            .block_on(fixture.controller.close())
            .unwrap();
    }
}

impl Transport for Lost {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Future<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let lose = if matches!(request.command, Command::CreateSession { .. }) {
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
fn preserves_uncertain_creation(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let lost = Arc::new(Lost {
            inner: fixture.transport.clone(),
            ids: Mutex::new(vec![]),
            release: tokio::sync::Semaphore::new(0),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(lost.clone()));
        let (view, visual) = open_session(cx, binding, None);
        wait(visual, |cx| {
            view.read(cx).node.connected && view.read(cx).defaults.connected
        });
        choose(visual, &view, fixture.target.id(), fixture.provider.id);
        click(visual, "live-chat-input");
        visual.simulate_input("First message");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx)
                .node
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.sessions.len() == 1)
        });
        assert!(view.read_with(visual, |view, _| view.pending));
        let source = view.read_with(visual, |view, _| crate::conversation::models::Selection {
            channel: view.provider_ids[&(
                fixture.binding.defaults.target(),
                fixture.source_provider.id,
            )],
            model: "fixture".into(),
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.select_model(&source, Effort::Default, window, cx)
            })
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config_owner),
            fixture.target.id()
        );
        lost.release.add_permits(1);
        wait(visual, |cx| {
            view.read(cx).error == Some("chat_action_unknown")
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.select_model(&source, Effort::Default, window, cx)
            })
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config_owner),
            fixture.target.id()
        );
        click(visual, "live-chat-input");
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("New draft while waiting");
        click(visual, "chat-retry");
        completed(visual, &view, 1);
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "New draft while waiting");
            fixture.assert_execution(view.session.as_ref().unwrap(), 1);
        });
        let ids = lost.ids.lock().unwrap().clone();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1]);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 1);
        visual.update(|window, _| window.remove_window());
        fixture
            .runtime
            .block_on(fixture.source.take().unwrap().shutdown())
            .unwrap();
        fixture.runtime.block_on(fixture.target.shutdown()).unwrap();
        fixture
            .runtime
            .block_on(fixture.controller.close())
            .unwrap();
    }
}
