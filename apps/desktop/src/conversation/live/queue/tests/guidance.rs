use super::*;

/// Starts a task whose model response waits for `release`.
fn running(
    fixture: &mut Fixture,
    release: &CancellationToken,
) -> (support::Server, sailry_protocol::QueuedTurn) {
    let server = fixture.runtime.block_on(support::Server::markdown_held(
        "Natural answer".into(),
        release.clone(),
    ));
    let provider = provider(&server.endpoint, "guide-model");
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

#[gpui::test]
fn guides_without_stopping(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let release = CancellationToken::new();
        let (server, active) = running(&mut fixture, &release);
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
        click(visual, selector("guide", selected));
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).retry.is_none()
                && panel.read(cx).queue.items[0].guidance == Some(active.id)
        });
        let guided = panel.read_with(visual, |panel, _| panel.queue.items[0].turn);
        assert_ne!(guided, selected);
        view.read_with(visual, |view, cx| {
            assert_eq!(view.active(), Some(active.id));
            assert_eq!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .runs
                    .iter()
                    .find(|run| run.turn == active.id)
                    .unwrap()
                    .status,
                Status::Running
            );
            assert_eq!(view.input.read(cx).value(), "Keep this composer draft");
        });
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        release.cancel();
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
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "Keep this composer draft");
            let users: Vec<_> = view
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .entries
                .iter()
                .filter(|entry| entry.author == "user")
                .map(|entry| entry.turn)
                .collect();
            assert_eq!(users, vec![active.id, guided]);
        });
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn enter_guides_running_task(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let release = CancellationToken::new();
        let (server, active) = running(&mut fixture, &release);
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
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Prefer the smaller fix");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            !view.read(cx).pending
                && view.read(cx).outgoing.is_none()
                && panel
                    .read(cx)
                    .queue
                    .items
                    .first()
                    .is_some_and(|item| item.guidance == Some(active.id))
        });
        let guided = panel.read_with(visual, |panel, _| panel.queue.items[0].turn);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(selector("guided", guided)).is_some());
        assert!(visual.debug_bounds(selector("guide", guided)).is_none());
        assert!(visual.debug_bounds("live-outgoing-message").is_none());
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            ""
        );
        release.cancel();
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .entries
                        .iter()
                        .filter(|entry| entry.author == "user")
                        .map(|entry| entry.turn)
                        .eq([active.id, guided])
                        && snapshot.page.runs.iter().all(|run| {
                            !matches!(
                                run.status,
                                Status::Queued | Status::Running | Status::Stopping
                            )
                        })
                })
        });
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
