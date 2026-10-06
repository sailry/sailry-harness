use super::*;

const PROMPT: &str = "Answer this fixture task";
const ANSWER: &str = "answer-approval-model";

fn configure(fixture: &fixture::Fixture) {
    let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
        panic!("snapshot expected")
    };
    let mut provider = snapshot
        .providers
        .into_iter()
        .find(|provider| provider.id == fixture.session.config.provider)
        .unwrap();
    provider.api = ModelApi::Responses;
    fixture.execute(Command::PutProvider {
        expected_revision: provider.revision,
        provider,
    });
}

fn terminal(view: &View) -> bool {
    view.history.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.page.runs.first().is_some_and(|run| {
            !matches!(
                run.status,
                Status::Queued | Status::Running | Status::Stopping
            )
        })
    })
}

fn submit(view: &Entity<View>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        view.read(cx).connected() && view.read(cx).configured()
    });
    fixture::tap(visual, "live-chat-input");
    visual.simulate_input(PROMPT);
    visual.simulate_keystrokes("enter");
    wait(visual, |cx| terminal(view.read(cx)));
}

fn bounds(visual: &mut VisualTestContext, selector: &str) -> Option<Bounds<Pixels>> {
    visual.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}

fn answer(view: &Entity<View>, visual: &mut VisualTestContext) {
    let (turn, text) = view.read_with(visual, |view, _| {
        let snapshot = view.history.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.page.runs.len(), 1);
        let run = &snapshot.page.runs[0];
        assert_eq!(run.status, Status::Completed);
        assert!(run.error.is_none());
        assert!(snapshot.drafts.is_empty());
        let entries: Vec<_> = snapshot
            .page
            .entries
            .iter()
            .filter(|entry| entry.author != "user")
            .collect();
        let parts: Vec<_> = entries.iter().flat_map(|entry| &entry.parts).collect();
        assert_eq!(parts, vec![&Part::Text(ANSWER.into())]);
        let key = format!("{}-{}-0", run.turn, entries[0].id);
        (run.turn, view.texts.borrow().get(&key).unwrap().clone())
    });
    assert!(bounds(visual, &format!("live-turn-text-{turn}")).is_some());
    assert!(bounds(visual, &format!("live-turn-status-{turn}-turn_completed")).is_some());
    assert!(bounds(visual, &format!("live-turn-status-{turn}-turn_failed")).is_none());
    text.update(visual, |text, cx| {
        text.select_all(cx);
        assert_eq!(text.selected_text(), ANSWER);
    });
}

fn failure(view: &Entity<View>, visual: &mut VisualTestContext) {
    let turn = view.read_with(visual, |view, _| {
        let snapshot = view.history.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.page.runs.len(), 1);
        let run = &snapshot.page.runs[0];
        assert_eq!(run.status, Status::Failed);
        assert_eq!(
            run.error.as_ref().unwrap().message,
            "Model returned no answer"
        );
        assert!(snapshot.drafts.is_empty());
        assert!(snapshot.page.entries.iter().any(|entry| {
            entry.author == "user"
                && entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::Text(text) if text == PROMPT))
        }));
        assert!(
            snapshot
                .page
                .entries
                .iter()
                .filter(|entry| entry.author != "user")
                .flat_map(|entry| &entry.parts)
                .all(|part| !matches!(part, Part::Text(_)))
        );
        run.turn
    });
    assert!(bounds(visual, &format!("live-turn-status-{turn}-turn_failed")).is_some());
    assert!(bounds(visual, &format!("live-turn-status-{turn}-turn_completed")).is_none());
    assert!(bounds(visual, &format!("live-turn-text-{turn}")).is_none());
    assert!(bounds(visual, &format!("live-turn-retry-{turn}")).is_some());
}

#[gpui::test]
fn renders_final_payload(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        // The Responses fixture emits only response.completed, without text deltas.
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::start(false))
        });
        configure(&fixture);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        submit(&view, visual);
        answer(&view, visual);
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| terminal(view.read(cx)));
        answer(&view, visual);
        assert_eq!(
            fixture.task_requests(),
            1,
            "restore must not replay the turn"
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn fails_without_an_answer(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::empty_response())
        });
        configure(&fixture);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        submit(&view, visual);
        failure(&view, visual);
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| terminal(view.read(cx)));
        failure(&view, visual);
        assert_eq!(
            fixture.task_requests(),
            1,
            "restore must not replay the turn"
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
