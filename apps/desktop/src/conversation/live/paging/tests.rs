use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, open, tap},
    wait,
};
use core::prelude::v1::test;
use sailry_link::Transport;
use sailry_protocol::*;
use std::time::{Duration, Instant};

#[path = "../../../../../../crates/node-runtime/tests/agent/paging/transport.rs"]
pub(in crate::conversation::live) mod transport;
use transport::Observed;

#[test]
fn preserves_page_overlap() {
    let turns: Vec<_> = (0..6).map(|_| TurnId::new()).collect();
    assert_eq!(insertion(&turns[2..4], &turns), Some((2, 2)));
    assert_eq!(insertion(&[], &turns), Some((0, 6)));
    assert_eq!(insertion(&turns, &turns), Some((0, 0)));
    assert_eq!(insertion(&turns[2..], &turns), Some((2, 0)));
    assert_eq!(insertion(&turns[..4], &turns), Some((0, 2)));
    assert_eq!(insertion(&turns, &turns[1..]), None);
}

fn submit(fixture: &Fixture, index: usize) -> TurnId {
    let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: 1,
        message: format!("History {index}\n中文 🙂").into(),
    }) else {
        panic!("turn expected")
    };
    turn.id
}

fn complete(fixture: &Fixture, turn: TurnId) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let history = fixture
            .runtime
            .block_on(
                fixture
                    .binding
                    .client
                    .read_conversation(fixture.session.id, None, 1),
            )
            .unwrap();
        if let Some(run) = history
            .page
            .runs
            .iter()
            .find(|run| run.turn == turn && run.status == Status::Completed)
        {
            assert_eq!(run.turn, turn);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "history fixture completion deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn relative_y(cx: &mut VisualTestContext, selector: &str) -> Pixels {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let row = cx
        .debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
        .unwrap();
    row.origin.y - cx.debug_bounds("live-history-viewport").unwrap().origin.y
}

#[gpui::test]
fn preserves_reader_and_draft(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let mut turns = Vec::new();
        for index in 0..45 {
            let turn = submit(&fixture, index);
            complete(&fixture, turn);
            turns.push(turn);
        }
        let observed = Arc::new(Observed::new(fixture.transport.clone()));
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(
            view.read_with(visual, |view, _| view.rows.clone()),
            turns[25..]
        );
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep this draft 中文 🙂");
        view.update(visual, |view, cx| {
            view.scroller.update(cx, |scroller, cx| {
                scroller.scroll_to_item(0, cx);
            })
        });
        visual.run_until_parked();
        let selector = format!("live-turn-{}", turns[25]);
        let anchor = relative_y(visual, &selector);
        wait(visual, |cx| view.read(cx).history.older_error.is_some());
        assert!(
            (relative_y(visual, &selector) - anchor).abs() < px(1.),
            "anchor={anchor:?}, actual={:?}",
            relative_y(visual, &selector)
        );
        assert_eq!(observed.reads(), 1);
        tap(visual, "live-load-older");
        wait(visual, |cx| view.read(cx).history.loading_older);
        fixture.runtime.block_on(observed.wait_held());
        view.update(visual, |view, cx| view.load_older(cx));
        let live = submit(&fixture, 45);
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .last()
                        .is_some_and(|run| run.turn == live && run.status == Status::Completed)
                })
        });
        assert!(
            (relative_y(visual, &selector) - anchor).abs() < px(1.),
            "anchor={anchor:?}, actual={:?}",
            relative_y(visual, &selector)
        );
        assert_eq!(observed.reads(), 2);
        observed.release();
        wait(visual, |cx| {
            view.read(cx).rows.len() == 41 && !view.read(cx).history.loading_older
        });
        assert!(
            (relative_y(visual, &selector) - anchor).abs() < px(1.),
            "anchor={anchor:?}, actual={:?}",
            relative_y(visual, &selector)
        );
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
            "Keep this draft 中文 🙂"
        );
        assert!(!view.read_with(visual, |view, cx| {
            view.scroller.read(cx).is_following_tail()
        }));
        view.update(visual, |view, cx| {
            view.scroller.update(cx, |scroller, cx| {
                scroller.scroll_to_item(0, cx);
            })
        });
        visual.run_until_parked();
        let selector = format!("live-turn-{}", turns[5]);
        let anchor = relative_y(visual, &selector);
        wait(visual, |cx| {
            view.read(cx).rows.len() == 46 && !view.read(cx).history.loading_older
        });
        crate::feedback::tests::settle(visual);
        assert!(visual.debug_bounds("live-load-older").is_none());
        assert!(
            (relative_y(visual, &selector) - anchor).abs() < px(1.),
            "anchor={anchor:?}, actual={:?}",
            relative_y(visual, &selector)
        );
        assert_eq!(observed.cancelled_reads(), 0);
        observed.disconnect();
        wait(visual, |cx| !view.read(cx).history.connected);
        assert!(visual.debug_bounds("chat-connection-status").is_none());
        assert!(visual.debug_bounds("chat-unavailable").is_none());
        assert!(visual.debug_bounds("live-history-viewport").is_some());
        assert!(visual.debug_bounds("chat-sync-retry").is_none());
        assert!(visual.debug_bounds("chat-connection-reason").is_none());
        crate::feedback::tests::settle(visual);
        assert!(visual.debug_bounds("error-toast-detail").is_some());
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
            "Keep this draft 中文 🙂"
        );
        let last = submit(&fixture, 46);
        complete(&fixture, last);
        wait(visual, |cx| {
            view.read(cx).history.connected && view.read(cx).rows.len() == 47
        });
        assert!(visual.debug_bounds("chat-connection-status").is_none());
        assert!(
            (relative_y(visual, &selector) - anchor).abs() < px(1.),
            "anchor={anchor:?}, actual={:?}",
            relative_y(visual, &selector)
        );
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
            "Keep this draft 中文 🙂"
        );
        visual.executor().advance_clock(Duration::from_millis(300));
        visual.run_until_parked();
        tap(visual, "live-history-latest");
        assert!(view.read_with(visual, |view, cx| {
            view.scroller.read(cx).is_following_tail()
        }));
        let last = submit(&fixture, 47);
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .last()
                        .is_some_and(|run| run.turn == last && run.status == Status::Completed)
                })
        });
        assert!(
            visual
                .debug_bounds(Box::leak(format!("live-turn-{last}").into_boxed_str()))
                .is_some()
        );
        assert_eq!(fixture.task_requests(), 48);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn preserves_remount_height(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::with_tools(false, vec![]);
    let mut turns = Vec::new();
    for index in 0..6 {
        let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: 1,
            message: format!("History {index}\n\n{}", "Long document paragraph with Unicode 中文 🙂 and enough content for asynchronous parsing.\n\n".repeat(80)).into(),
        }) else { panic!("turn expected") };
        complete(&fixture, turn.id);
        turns.push(turn.id);
    }
    let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
    wait(visual, |cx| view.read(cx).connected());
    let scroll = |visual: &mut VisualTestContext, index| {
        view.update(visual, |view, cx| {
            view.scroller.update(cx, |scroller, cx| {
                scroller.scroll_to_item(index, cx);
            })
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    };
    scroll(visual, 2);
    let selector = Box::leak(format!("live-turn-{}", turns[2]).into_boxed_str());
    wait(visual, |_| true);
    // Let the first asynchronous parse finish before recording its measured height.
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(20));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    }
    scroll(visual, 2);
    let original = visual.debug_bounds(selector).unwrap();
    assert!(original.size.height > px(1000.));
    scroll(visual, 5);
    assert!(visual.debug_bounds(selector).is_none());
    scroll(visual, 2);
    let remounted = visual.debug_bounds(selector).unwrap();
    assert!(
        (original.size.height - remounted.size.height).abs() < px(1.),
        "remount must retain the parsed document height"
    );
    assert!(
        (original.origin.y - remounted.origin.y).abs() < px(1.),
        "remount must retain the reading anchor"
    );
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

#[gpui::test]
fn upward_scrolling(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::with_tools(false, vec![]);
    let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
        session: fixture.session.id, expected_revision: 1,
        message: "Long document paragraph with Unicode 中文 🙂 and enough content for asynchronous parsing.\n\n".repeat(160).into(),
    }) else { panic!("turn expected") };
    complete(&fixture, turn.id);
    let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
    wait(visual, |cx| view.read(cx).connected());
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(20));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    }
    view.update(visual, |view, cx| {
        view.scroller
            .update(cx, |state, cx| state.scroll_to_end(cx))
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let selector = Box::leak(format!("live-turn-{}", turn.id).into_boxed_str());
    let mut previous = visual.debug_bounds(selector).unwrap();
    let viewport = visual.debug_bounds("live-history-viewport").unwrap();
    for step in 0..24 {
        visual.simulate_event(ScrollWheelEvent {
            position: viewport.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(24.))),
            touch_phase: TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let current = visual.debug_bounds(selector).unwrap();
        assert!(
            (current.size.height - previous.size.height).abs() < px(1.),
            "height changed at step {step}: {previous:?} -> {current:?}"
        );
        assert!(
            (current.origin.y - previous.origin.y - px(24.)).abs() < px(1.),
            "wheel jumped at step {step}: {previous:?} -> {current:?}"
        );
        previous = current;
    }
}
