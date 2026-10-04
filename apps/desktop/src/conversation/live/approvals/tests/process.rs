use super::*;
use sailry_client::conversation::tools::State;
use sailry_protocol::process::{Completion, Outcome};

fn result(view: &View, index: usize) -> Completion {
    let value = view.history.calls[index]
        .result(&view.history.snapshot.as_ref().unwrap().page)
        .unwrap();
    let Output::CommandResult(result) = serde_json::from_value(value.clone()).unwrap() else {
        panic!("command result expected")
    };
    result
}

fn approve(view: &Entity<View>, visual: &mut VisualTestContext) -> ApprovalId {
    wait(visual, |cx| {
        pending(view, cx).is_some() && !view.read(cx).approvals.pending
    });
    let id = view.read_with(visual, |_, cx| pending(view, cx).unwrap());
    tap(visual, &format!("live-approval-approve-{id}"));
    id
}

#[gpui::test]
fn executes_reopens_and_denies(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"command": "printf '完整内容 中文 🙂\\n```rust\\n' > '资料.txt'\ncat '资料.txt'\nprintf 'failure detail\\377' >&2\nexit 7", "cwd": "nested", "timeout_ms": 5_000}),
                ),
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"command": "printf 'before timeout\\n'; sleep 30", "timeout_ms": 100}),
                ),
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"command": "printf never > denied.txt"}),
                ),
            ],
        );
        let root = fixture.directory.path().join("project");
        std::fs::create_dir(root.join("nested")).unwrap();
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-description-{id}").into_boxed_str()
                ))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-content-{id}").into_boxed_str()
                ))
                .is_none()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-details-{id}").into_boxed_str()
                ))
                .is_none()
        );
        assert!(!root.join("nested/资料.txt").exists());
        visual.simulate_keystrokes("escape");
        visual.executor().advance_clock(Duration::from_millis(400));
        visual.run_until_parked();
        tap(visual, "live-chat-input");
        visual.simulate_input("keep next draft 中文");
        tap(visual, "live-approval-trigger");
        approve(&view, visual);
        wait(visual, |cx| {
            pending(&view, cx).is_some_and(|next| next != id) && !view.read(cx).approvals.pending
        });
        view.read_with(visual, |view, cx| {
            let result = result(view, 0);
            assert_eq!(result.outcome, Outcome::Exited(7));
            assert_eq!(result.stdout.text, "完整内容 中文 🙂\n```rust\n");
            assert!(result.stderr.invalid_utf8);
            assert!(result.stderr.text.starts_with("failure detail"));
            assert_eq!(view.input.read(cx).value(), "keep next draft 中文");
        });
        assert_eq!(
            std::fs::read_to_string(root.join("nested/资料.txt")).unwrap(),
            "完整内容 中文 🙂\n```rust\n"
        );
        let second = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            pending(&view, cx) == Some(second) && view.read(cx).connected()
        });
        approve(&view, visual);
        wait(visual, |cx| {
            pending(&view, cx).is_some_and(|next| next != second)
                && !view.read(cx).approvals.pending
        });
        view.read_with(visual, |view, _| {
            assert_eq!(result(view, 1).outcome, Outcome::TimedOut);
            assert_eq!(result(view, 1).stdout.text, "before timeout\n");
        });
        let third = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        tap(visual, &format!("live-approval-reject-{third}"));
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).approvals.pending
        });
        let (turn, group, first) = view.read_with(visual, |view, _| {
            assert_eq!(
                view.history.calls[2].approval.as_ref().unwrap().state,
                ApprovalState::Denied
            );
            let call = &view.history.calls[0];
            (
                call.turn,
                format!("tools-{}", call.source.key()),
                call.source.key(),
            )
        });
        tap(visual, &format!("live-turn-work-{turn}"));
        tap(visual, &format!("live-{turn}-tool-group-{first}"));
        assert!(
            visual
                .debug_bounds(Box::leak(format!("live-{turn}-{group}").into_boxed_str()))
                .is_some()
        );
        tap(visual, &format!("live-{turn}-{group}"));
        for _ in 0..8 {
            visual.executor().advance_clock(Duration::from_millis(100));
            visual.run_until_parked();
        }
        visual.update(|window, cx| window.draw(cx).clear(cx));
        for index in 0..2 {
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("tool-content-{turn}-{first}-{index}").into_boxed_str()
                    ))
                    .is_some()
            );
        }
        assert!(!root.join("denied.txt").exists());
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 4);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn stops_without_a_tool_result(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("commands", "run_command"),
                json!({"command": "printf ready > ready.txt; sleep 30; printf unexpected > after.txt"}),
            )],
        );
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        approve(&view, visual);
        wait(visual, |_| {
            fixture.directory.path().join("project/ready.txt").exists()
        });
        let (turn, key) = view.read_with(visual, |view, _| {
            let call = &view.history.calls[0];
            (call.turn, call.source.key())
        });
        let details = format!("live-tool-details-{turn}-{key}");
        wait(visual, |cx| {
            view.read(cx).history.calls[0]
                .approval
                .as_ref()
                .unwrap()
                .state
                == ApprovalState::Approved
        });
        visual.executor().advance_clock(Duration::from_millis(400));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds(Box::leak(details.clone().into_boxed_str()))
                .is_none(),
            "running tool without output stays closed"
        );
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).pending
        });
        view.read_with(visual, |view, _| {
            let call = &view.history.calls[0];
            assert_eq!(
                call.approval.as_ref().unwrap().state,
                ApprovalState::Approved
            );
            assert_eq!(call.state, State::Interrupted);
            assert!(
                call.result(&view.history.snapshot.as_ref().unwrap().page)
                    .is_none()
            );
        });
        visual.executor().advance_clock(Duration::from_millis(400));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds(Box::leak(details.into_boxed_str()))
                .is_none(),
            "stopped tool without a result stays closed"
        );
        tap(visual, &format!("live-turn-work-{turn}"));
        tap(visual, &format!("live-{turn}-tools-{key}"));
        visual.executor().advance_clock(Duration::from_millis(400));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-tool-details-{turn}-{key}").into_boxed_str()
                ))
                .is_none(),
            "a tool without a result has nothing to expand"
        );
        assert!(!fixture.directory.path().join("project/after.txt").exists());
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
