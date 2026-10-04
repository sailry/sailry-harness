use super::*;
use sailry_protocol::conversation::progress::State;
use serde_json::json;

fn calls(long: bool) -> Vec<(String, serde_json::Value)> {
    let mut calls: Vec<_> = ["in_progress", "completed"].into_iter().map(|state| {
        (crate::agent_fixture::plugin_tool("progress", "update_plan"), json!({"title": "Task progress 中文 🙂", "steps": if long {
            (0..48).map(|index| json!({"description": format!("Step {index}: inspect the relevant files, preserve edits, and verify the result 中文 🙂"), "state": state})).collect::<Vec<_>>()
        } else {
            vec![json!({"description": "Inspect 中文 🙂", "state": "completed"}), json!({"description": "Verify", "state": state}), json!({"description": "Publish", "state": "skipped"})]
        }}))
    }).chain(std::iter::once((
        crate::agent_fixture::plugin_tool("files", "write_file"),
        json!({"path": "verified.txt", "text": "done", "expected_revision": null}),
    ))).collect();
    calls.swap(1, 2);
    calls
}

fn awaiting_approval(view: &Entity<View>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        view.read(cx)
            .history
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot.page.approvals.iter().any(|approval| {
                    approval.state == sailry_protocol::conversation::ApprovalState::Pending
                })
            })
    });
    visual.simulate_keystrokes("escape");
    settle(visual);
}

fn completed(view: &Entity<View>, visual: &mut VisualTestContext) {
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
                    .any(|run| run.status == Status::Completed)
            })
    });
}

fn key(view: &Entity<View>, visual: &VisualTestContext) -> (String, String) {
    view.read_with(visual, |view, _| {
        let call = view
            .history
            .calls
            .iter()
            .rev()
            .find(|call| call.progress.is_some())
            .unwrap();
        let key = format!("{}-{}", call.turn, call.source.key());
        (
            format!("live-{}-tools-{}", call.turn, call.source.key()),
            key,
        )
    })
}

fn settle(visual: &mut VisualTestContext) {
    for _ in 0..8 {
        visual.executor().advance_clock(Duration::from_millis(100));
        visual.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.refresh();
        });
        wait(visual, |_| true);
    }
}

#[gpui::test]
fn restores_history(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, calls(false));
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input("Unsent draft 中文 🙂");
        fixture.start();
        awaiting_approval(&view, visual);
        fixture::tap(visual, "composer-todo");
        assert!(visual.debug_bounds("live-progress-composer-todo").is_some());
        assert!(
            visual
                .debug_bounds("live-progress-card-composer-todo")
                .is_none()
        );
        assert!(
            visual
                .debug_bounds("live-progress-heading-composer-todo")
                .is_none()
        );
        visual.simulate_keystrokes("escape");
        settle(visual);
        let approval = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.approvals[0].id
        });
        fixture::tap(visual, "live-approval-trigger");
        fixture::tap(visual, &format!("live-approval-approve-{approval}"));
        completed(&view, visual);
        assert!(visual.debug_bounds("composer-todo").is_none());
        let turn = view.read_with(visual, |view, _| view.history.calls[0].turn);
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        let (toggle, key) = key(&view, visual);
        assert!(
            visual
                .debug_bounds(Box::leak(toggle.into_boxed_str()))
                .is_none()
        );
        settle(visual);
        assert!(
            visual
                .debug_bounds(Box::leak(format!("live-progress-{key}").into_boxed_str()))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-progress-card-{key}").into_boxed_str()
                ))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-progress-heading-{key}").into_boxed_str()
                ))
                .is_none()
        );
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "Unsent draft 中文 🙂");
            assert_eq!(
                view.history.calls[0].progress.as_ref().unwrap().steps[1].state,
                State::InProgress
            );
            assert_eq!(
                view.history.calls[2].progress.as_ref().unwrap().steps[1].state,
                State::Completed
            );
            assert!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .questions
                    .is_empty()
            );
        });
        click(visual, "live-chat-input");
        visual.simulate_input(" retained");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Unsent draft 中文 🙂 retained"
        );
        let expected = view.read_with(visual, |view, _| view.history.calls.clone());
        visual.update(|window, _| window.remove_window());
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        completed(&view, visual);
        assert_eq!(
            view.read_with(visual, |view, _| view.history.calls.clone()),
            expected
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 4);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn long_todo_scrolling(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, calls(true));
        fixture.start();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        awaiting_approval(&view, visual);
        fixture::tap(visual, "composer-todo");
        settle(visual);
        let list = "live-progress-composer-todo";
        let last = "progress-composer-todo-47-label";
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            for width in [480., 1000.] {
                let handle = visual.update(|window, cx| {
                    Theme::change(mode, Some(window), cx);
                    window.window_handle()
                });
                visual.simulate_window_resize(handle, size(px(width), px(820.)));
                settle(visual);
                let bounds = visual.debug_bounds(list).unwrap();
                assert!(bounds.left() >= px(0.) && bounds.right() <= px(width));
                assert!(bounds.size.height <= px(336.));
                let target = bounds.center();
                visual.simulate_mouse_move(target, None, Modifiers::default());
                settle(visual);
                visual.simulate_event(ScrollWheelEvent {
                    position: target,
                    delta: ScrollDelta::Pixels(point(px(0.), px(-20000.))),
                    touch_phase: TouchPhase::Moved,
                    modifiers: Modifiers::default(),
                });
                settle(visual);
                let item = visual.debug_bounds(last).unwrap();
                assert!(
                    item.bottom() <= bounds.bottom() + px(1.) && item.bottom() >= bounds.top(),
                    "item {item:?}, list {bounds:?}"
                );
                assert!(item.right() <= bounds.right());
                assert!(
                    item.size.height <= px(24.),
                    "task labels must stay on one line: {item:?}"
                );
                assert!(visual.debug_bounds("live-chat-input").is_some());
            }
        }
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
