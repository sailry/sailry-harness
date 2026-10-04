use super::*;
use core::prelude::v1::test;
use sailry_protocol::{SessionId, TurnId};

fn run() -> Run {
    Run {
        worktree: sailry_protocol::WorktreeId::new(),
        turn: TurnId::new(),
        kind: RunKind::Task,
        session: SessionId::new(),
        sequence: 1,
        revision: 1,
        status: Status::Running,
        error: None,
        origin: None,
        started_ms: Some(1000),
        finished_ms: None,
    }
}

#[test]
fn timestamp_dates() {
    use chrono::TimeZone;
    let now = chrono::Local
        .with_ymd_and_hms(2026, 9, 14, 20, 6, 3)
        .unwrap();
    assert_eq!(timestamp(now, now), "20:06");
    assert_eq!(
        timestamp(now - chrono::Duration::days(1), now),
        "09-13 20:06"
    );
    let previous = chrono::Local
        .with_ymd_and_hms(2025, 9, 14, 20, 6, 3)
        .unwrap();
    assert_eq!(timestamp(previous, now), "2025-09-14 20:06");
}

#[test]
fn elapsed_execution_time() {
    let mut run = run();
    for (now, expected) in [
        (500, "0s"),
        (2000, "1s"),
        (61000, "1m00s"),
        (3602000, "1h00m01s"),
    ] {
        assert_eq!(elapsed(&run, now).as_deref(), Some(expected));
    }
    run.status = Status::Completed;
    run.finished_ms = Some(71000);
    assert_eq!(elapsed(&run, 900000).as_deref(), Some("1m10s"));
    assert!(finished(&run).is_some());
    run.status = Status::Interrupted;
    run.finished_ms = None;
    assert_eq!(elapsed(&run, 900000), None);
    assert_eq!(finished(&run), None);
}

#[test]
fn active_phase_lifecycle() {
    let mut run = run();
    let page = Page {
        session: run.session,
        revision: 1,
        entries: vec![],
        runs: vec![],
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        next_before: None,
    };
    assert_eq!(phase(&run, &page, &[]), Some("turn_awaiting_response"));
    let reasoning = [Block::Thinking(
        "reasoning".into(),
        "Thinking details".into(),
    )];
    assert_eq!(phase(&run, &page, &reasoning), Some("turn_thinking"));
    let text = [Block::Text("response".into(), "Partial answer".into())];
    assert_eq!(phase(&run, &page, &text), Some("turn_generating"));
    let activity = [Block::Tools(vec![])];
    assert!(continuing_tools(&run, &page, &activity));
    assert_eq!(phase(&run, &page, &activity), None);
    let next = [
        Block::Tools(vec![]),
        Block::Thinking("next".into(), "New reasoning".into()),
    ];
    assert!(!continuing_tools(&run, &page, &next));
    assert_eq!(phase(&run, &page, &next), Some("turn_thinking"));
    let retry = [Block::Tools(vec![]), Block::Retry(1, 3)];
    assert!(!continuing_tools(&run, &page, &retry));
    assert_eq!(phase(&run, &page, &retry), None);
    for status in [
        Status::Completed,
        Status::Failed,
        Status::Cancelled,
        Status::Interrupted,
    ] {
        run.status = status;
        assert!(!continuing_tools(&run, &page, &activity));
        assert_eq!(phase(&run, &page, &[]), None);
        assert_eq!(phase(&run, &page, &text), None);
    }
}

#[gpui_kit::test]
fn preserves_frame_regions(cx: &mut gpui_kit::TestAppContext) {
    use crate::conversation::live::{
        tests::{fixture, wait},
        *,
    };
    fixture::init(cx);
    for remote in [false, true] {
        let start = Arc::new(tokio::sync::Notify::new());
        let finish = Arc::new(tokio::sync::Notify::new());
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::held(
                start.clone(),
                finish.clone(),
            ))
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input("Frame layout fixture");
        visual.simulate_keystrokes("enter");
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
                        .any(|run| run.status == Status::Running)
                })
        });
        let run = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.runs[0].clone()
        });
        let selector = |part: &str| {
            Box::leak(format!("live-turn-{part}-{}", run.turn).into_boxed_str()) as &'static str
        };
        let header =
            Box::leak(format!("live-turn-status-{}-chat_running", run.turn).into_boxed_str());
        let thinking = Box::leak(
            format!("live-turn-phase-{}-turn_awaiting_response", run.turn).into_boxed_str(),
        );
        let generating =
            Box::leak(format!("live-turn-phase-{}-turn_generating", run.turn).into_boxed_str());
        let avatar = selector("avatar");
        let avatar_bounds = visual.debug_bounds(avatar).unwrap();
        let phase_bounds = visual.debug_bounds(thinking).unwrap();
        assert_eq!(avatar_bounds.size, size(px(20.), px(20.)));
        assert!(phase_bounds.contains(&avatar_bounds.center()));
        assert!(
            visual.debug_bounds(header).unwrap().bottom()
                < visual.debug_bounds(thinking).unwrap().top()
        );
        assert!(visual.debug_bounds(selector("footer")).is_none());
        assert!(run.started_ms.is_some());
        assert!(view.read_with(visual, |view, _| view.clock.is_some()));
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.history.connected = false;
                view.history.error = None;
                cx.notify();
            })
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("chat_connecting").is_none());
        assert!(visual.debug_bounds("chat_reconnecting").is_none());
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.history.connected = false;
                view.history.error = Some(sailry_protocol::Fault::new(
                    sailry_protocol::ErrorCode::Unavailable,
                    "fixture reconnect",
                ));
                cx.notify();
            })
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("chat_reconnecting").is_none());
        assert!(visual.debug_bounds("chat-connection-status").is_none());
        assert!(visual.debug_bounds("chat-connection-details").is_none());
        // Inspect the injected offline frame before advancing the live subscription.
        // Recovery tests exercise the toast's rendered details and retry action.
        assert_eq!(
            visual.update(|window, cx| window.notifications(cx).len()),
            1
        );
        assert!(visual.debug_bounds(header).is_none());
        let unsynced = Box::leak(
            format!("live-turn-status-{}-chat_status_unsynced", run.turn).into_boxed_str(),
        );
        assert!(visual.debug_bounds(unsynced).is_some());
        assert!(visual.debug_bounds(thinking).is_none());
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.history.connected = true;
                view.history.error = None;
                cx.notify();
            })
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("chat_reconnecting").is_none());
        assert!(visual.debug_bounds("chat-connection-status").is_none());
        start.notify_one();
        wait(visual, |cx| {
            !view
                .read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .drafts
                .is_empty()
        });
        assert!(visual.debug_bounds(thinking).is_none());
        assert!(visual.debug_bounds(avatar).is_some());
        assert!(
            visual.debug_bounds(selector("text")).unwrap().bottom()
                < visual.debug_bounds(generating).unwrap().top()
        );
        finish.notify_one();
        wait(visual, |cx| {
            view.read(cx).history.snapshot.as_ref().unwrap().page.runs[0].status
                == Status::Completed
        });
        let completed = view.read_with(visual, |view, _| {
            assert!(view.clock.is_none());
            view.history.snapshot.as_ref().unwrap().page.runs[0].clone()
        });
        assert!(completed.finished_ms.unwrap() >= run.started_ms.unwrap());
        assert!(visual.debug_bounds(generating).is_none());
        assert!(visual.debug_bounds(avatar).is_none());
        assert!(
            visual.debug_bounds(selector("text")).unwrap().bottom()
                < visual.debug_bounds(selector("footer")).unwrap().top()
        );
        visual.update(|window, _| window.remove_window());
        let (reopened, visual) =
            fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            reopened
                .read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !snapshot.page.runs.is_empty())
        });
        assert_eq!(
            reopened.read_with(visual, |view, _| view
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .runs[0]
                .clone()),
            completed
        );
        assert!(visual.debug_bounds(selector("footer")).is_some());
        assert!(visual.debug_bounds(avatar).is_none());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[test]
fn completion_precedence() {
    let mut run = run();
    assert_eq!(status(&run, false), "chat_status_unsynced");
    assert_eq!(status(&run, true), "chat_running");
    for terminal in [
        Status::Completed,
        Status::Cancelled,
        Status::Failed,
        Status::Interrupted,
    ] {
        run.status = terminal;
        assert_eq!(
            status(&run, false),
            super::super::super::subagents::status_key(terminal)
        );
    }
}
