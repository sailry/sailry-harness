use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, open, tap},
    wait,
};
use core::prelude::v1::test;
use std::sync::Mutex;

mod lifecycle;

fn complete(fixture: &Fixture, message: &str) -> TurnId {
    let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: 1,
        message: message.into(),
    }) else {
        panic!("turn expected")
    };
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
        if page
            .runs
            .iter()
            .any(|run| run.turn == turn.id && run.status == Status::Completed)
        {
            return turn.id;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "rewind fixture completion deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn choose(view: &Entity<View>, turn: TurnId, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let index = view.rows.iter().position(|id| *id == turn).unwrap();
        view.scroller
            .update(cx, |scroller, cx| scroller.scroll_to_item(index, cx));
    });
    tap(cx, &format!("live-turn-more-{turn}"));
    cx.simulate_keystrokes("down down enter");
    wait(cx, |cx| view.read(cx).connected());
}

#[gpui::test]
fn confirmation_preserves_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let first = complete(&fixture, "Retained 中文 🙂");
        let last = complete(&fixture, "Removed 中文 🙂");
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let before = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.clone()
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("Draft survives rewind 中文 🙂");
        view.update(visual, |view, _| {
            view.expanded.insert((last, "tool".into()), true);
        });
        choose(&view, first, visual);
        assert!(visual.has_pending_prompt());
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert_eq!(
            view.read_with(visual, |view, _| view
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .clone()),
            before
        );
        assert!(view.read_with(visual, |view, _| view.backup.is_none()));
        choose(&view, first, visual);
        crate::prompts::tests::answer(visual, "settings_cancel");
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(!visual.has_pending_prompt());
        let opened = Arc::new(Mutex::new(None));
        view.update(visual, |view, cx| {
            view.backup = Some(Session {
                id: SessionId::new(),
                ..fixture.session.clone()
            });
            let opened = opened.clone();
            cx.subscribe(&cx.entity(), move |_, _, event, _| {
                if let Event::Forked(session) = event {
                    *opened.lock().unwrap() = Some(session.id);
                }
            })
            .detach();
        });
        choose(&view, first, visual);
        crate::prompts::tests::answer(visual, "chat_rewind_action");
        wait(visual, |cx| {
            !view.read(cx).pending
                && view.read(cx).backup.is_none()
                && view
                    .read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .revision
                    == 2
                && view.read(cx).connected()
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(view.session(), Some(fixture.session.id));
            assert_eq!(view.input.read(cx).value(), "Draft survives rewind 中文 🙂");
            assert_eq!(view.rows, [first]);
            assert!(view.expanded.is_empty());
            assert!(view.retry.is_none());
            assert!(view.rewind_request(first).is_none());
            assert!(view.backup.is_none());
        });
        assert!(visual.debug_bounds("live-rewind-notice").is_none());
        assert_eq!(
            visual.update(|window, cx| {
                crate::feedback::tests::count(window, &tr("chat_rewound"), cx)
            }),
            0
        );
        assert!(visual.debug_bounds("live-rewind-backup").is_none());
        assert_eq!(*opened.lock().unwrap(), None);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 1);
        let (reopened, reopened_cx) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(reopened_cx, |cx| reopened.read(cx).connected());
        assert_eq!(
            reopened.read_with(reopened_cx, |view, _| view.rows.clone()),
            [first]
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        fixture.close();
    }
}
