use super::*;

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
        click(visual, selector("guide", selected));
        // The awaited response yields at once; guidance replaces the selected input.
        wait(visual, |cx| {
            !panel.read(cx).pending
                && panel.read(cx).queue.items.is_empty()
                && status(&view, active.id, cx) == Some(Status::Continued)
        });
        fixture.runtime.block_on(server.wait_count(2));
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
fn enter_guides_running_task(cx: &mut TestAppContext) {
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
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
