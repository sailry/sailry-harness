use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, open, tap},
    wait,
};
use core::prelude::v1::test;
use std::sync::Mutex;

mod lifecycle;

fn complete(fixture: &Fixture) -> TurnId {
    fixture.start();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let page = fixture
            .runtime
            .block_on(
                fixture
                    .binding
                    .client
                    .read_conversation(fixture.session.id, None, 20),
            )
            .unwrap()
            .page;
        if let Some(run) = page.runs.iter().find(|run| run.status == Status::Completed) {
            return run.turn;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fork fixture completion deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn observe(view: &Entity<View>, cx: &mut VisualTestContext) -> Arc<Mutex<Option<Session>>> {
    let result = Arc::new(Mutex::new(None));
    let target = result.clone();
    view.update(cx, |_, cx| {
        cx.subscribe(view, move |_, _, event, _| {
            if let Event::Forked(session) = event {
                *target.lock().unwrap() = Some(*session.clone());
            }
        })
        .detach();
    });
    result
}

fn choose(view: &Entity<View>, turn: TurnId, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let index = view.rows.iter().position(|id| *id == turn).unwrap();
        view.scroller
            .update(cx, |scroller, cx| scroller.scroll_to_item(index, cx));
    });
    tap(cx, &format!("live-turn-more-{turn}"));
    cx.simulate_keystrokes("down enter");
}

#[gpui::test]
fn preserves_source_context(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let turn = complete(&fixture);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let created = observe(&view, visual);
        tap(visual, "live-chat-input");
        visual.simulate_input("Preserved source draft 中文 🙂");
        let original = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.clone()
        });
        choose(&view, turn, visual);
        wait(visual, |_| created.lock().unwrap().is_some());
        let branch = created.lock().unwrap().clone().unwrap();
        assert_ne!(branch.id, fixture.session.id);
        view.read_with(visual, |view, cx| {
            assert_eq!(view.session(), Some(fixture.session.id));
            assert_eq!(
                view.input.read(cx).value(),
                "Preserved source draft 中文 🙂"
            );
            assert_eq!(view.history.snapshot.as_ref().unwrap().page, original);
            assert!(view.retry.is_none());
        });
        let (child, child_cx) = open(cx, fixture.binding.clone(), branch.clone());
        wait(child_cx, |cx| child.read(cx).connected());
        child.read_with(child_cx, |view, cx| {
            assert!(view.input.read(cx).value().is_empty());
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(page.entries, original.entries);
            assert_eq!(page.runs[0].origin, Some(fixture.session.id));
        });
        tap(child_cx, "live-chat-input");
        child_cx.simulate_input("Independent continuation 中文 🙂");
        child_cx.simulate_keystrokes("enter");
        wait(child_cx, |cx| {
            child
                .read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.page.runs.len() == 2
                        && snapshot.page.runs.last().unwrap().status == Status::Completed
                })
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        let source = fixture
            .runtime
            .block_on(
                fixture
                    .binding
                    .client
                    .read_conversation(fixture.session.id, None, 20),
            )
            .unwrap()
            .page;
        assert_eq!(source.entries, original.entries);
        fixture.close();
    }
}
