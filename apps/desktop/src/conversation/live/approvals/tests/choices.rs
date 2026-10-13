use super::*;
use crate::conversation::live::tests::fixture::press;
use sailry_protocol::conversation::{ApprovalChoice, ApprovalSource};

pub(super) fn choose(visual: &mut VisualTestContext, id: ApprovalId, action: &str, index: usize) {
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let selector = Box::leak(format!("live-approval-{action}-options-{id}").into_boxed_str());
    let bounds = visual
        .debug_bounds(selector)
        .expect("approval split control");
    let caret = point(bounds.right() - px(8.), bounds.center().y);
    visual.simulate_mouse_move(caret, None, Modifiers::default());
    visual.run_until_parked();
    press(visual, caret);
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    for _ in 0..=index {
        visual.simulate_keystrokes("down");
    }
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

pub(super) fn reason(visual: &mut VisualTestContext, id: ApprovalId, text: &str) {
    choose(visual, id, "reject", 0);
    tap(visual, &format!("live-approval-reason-{id}"));
    visual.simulate_input(text);
    visual.run_until_parked();
}

#[gpui::test]
fn remembers_session(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        view.read_with(visual, |view, _| {
            let approval = view.history.calls[0].approval.as_ref().unwrap();
            assert!(approval.can_remember);
            assert!(approval.prefix.is_none());
        });
        choose(visual, id, "approve", 0);
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).approvals.pending
        });
        view.read_with(visual, |view, _| {
            assert_eq!(view.history.calls.len(), 2);
            let approvals: Vec<_> = view
                .history
                .calls
                .iter()
                .map(|call| call.approval.as_ref().unwrap())
                .collect();
            assert_eq!(approvals[0].id, id);
            assert_eq!(approvals[0].source, ApprovalSource::User);
            assert_eq!(approvals[1].source, ApprovalSource::SessionRule);
            for approval in approvals {
                assert_eq!(approval.state, ApprovalState::Approved);
                assert_eq!(approval.choice, Some(ApprovalChoice::Session));
                assert_eq!(approval.session, fixture.session.id);
                assert!(approval.reason.is_none());
            }
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/资料.txt")).unwrap(),
            "完整内容 中文 🙂\n```rust\n"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/denied.txt")).unwrap(),
            "never"
        );
        assert_eq!(
            visual.update(|window, cx| crate::feedback::tests::count(window, "Allowed", cx)),
            1
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn allows_session_with_pointer(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;

    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            ["inventory", "notes"]
                .into_iter()
                .map(|path| {
                    (
                        crate::agent_fixture::plugin_tool("files", "create_directory"),
                        json!({"path": path}),
                    )
                })
                .collect(),
        );
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        let selector = Box::leak(format!("live-approval-approve-options-{id}").into_boxed_str());
        let bounds = visual
            .debug_bounds(selector)
            .expect("approval split control");
        let caret = point(bounds.right() - px(8.), bounds.center().y);
        visual.simulate_mouse_move(caret, None, Modifiers::default());
        visual.run_until_parked();
        press(visual, caret);
        visual.run_until_parked();
        visual.update(|window, cx| window.render_frame(cx));

        let menu = visual
            .debug_bounds("popup-menu")
            .expect("approval options menu");
        let panel = visual
            .debug_bounds("pending-approvals")
            .expect("pending approval panel");
        let item = menu.center();
        assert!(
            !panel.contains(&item),
            "menu item must extend outside the panel"
        );
        visual.simulate_mouse_move(item, None, Modifiers::default());
        visual.run_until_parked();
        visual.simulate_mouse_down(item, MouseButton::Left, Modifiers::default());
        visual.update(|window, cx| window.render_frame(cx));
        assert!(visual.debug_bounds("popup-menu").is_some());
        assert!(visual.debug_bounds("pending-approvals").is_some());
        assert_eq!(view.read_with(visual, |_, cx| pending(&view, cx)), Some(id));
        visual.simulate_mouse_up(item, MouseButton::Left, Modifiers::default());
        visual.update(|window, cx| window.render_frame(cx));

        wait(visual, |cx| {
            view.read(cx).active().is_none()
                && !view.read(cx).approvals.pending
                && pending(&view, cx).is_none()
        });
        let restored = super::lifecycle::restored(&fixture);
        assert_eq!(restored.page.approvals.len(), 2);
        for (approval, source) in restored
            .page
            .approvals
            .iter()
            .zip([ApprovalSource::User, ApprovalSource::SessionRule])
        {
            assert_eq!(approval.state, ApprovalState::Approved);
            assert_eq!(approval.source, source);
            assert_eq!(approval.choice, Some(ApprovalChoice::Session));
        }
        assert_eq!(restored.page.approvals[0].id, id);
        assert!(
            restored
                .page
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        for path in ["inventory", "notes"] {
            assert!(fixture.directory.path().join("project").join(path).is_dir());
        }
        assert!(visual.debug_bounds("pending-approvals").is_none());
        assert!(visual.debug_bounds("live-approval-trigger").is_none());
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[gpui::test]
fn admits_workspace_without_remembered_rules(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"command":"printf %s first"}),
                ),
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"command":"printf %s second"}),
                ),
            ],
        );
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
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
        view.read_with(visual, |view, _| {
            assert_eq!(view.history.calls.len(), 2);
            for (index, text) in ["first", "second"].into_iter().enumerate() {
                let call = &view.history.calls[index];
                let approval = call.approval.as_ref().unwrap();
                assert_eq!(approval.source, ApprovalSource::Sandbox);
                assert_eq!(
                    approval.access,
                    Some(sailry_protocol::process::Access::Workspace)
                );
                assert_eq!(approval.state, ApprovalState::Approved);
                assert!(approval.choice.is_none());
                assert!(!approval.can_remember);
                assert!(approval.prefix.is_none());
                let value = call
                    .result(&view.history.snapshot.as_ref().unwrap().page)
                    .unwrap();
                let Output::CommandResult(result) = serde_json::from_value(value.clone()).unwrap()
                else {
                    panic!("command result expected")
                };
                assert_eq!(result.stdout.text, text);
                assert_eq!(result.outcome, sailry_protocol::process::Outcome::Exited(0));
            }
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        assert!(visual.debug_bounds("pending-approvals").is_none());
        let restored = super::lifecycle::restored(&fixture);
        assert_eq!(restored.page.approvals.len(), 2);
        for approval in &restored.page.approvals {
            assert_eq!(approval.source, ApprovalSource::Sandbox);
            assert_eq!(
                approval.access,
                Some(sailry_protocol::process::Access::Workspace)
            );
            assert_eq!(approval.state, ApprovalState::Approved);
            assert!(!approval.can_remember);
        }
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn preserves_reason_draft(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep the conversation draft 中文 🙂");
        tap(visual, "live-approval-trigger");
        choose(visual, id, "reject", 0);
        tap(visual, &format!("live-approval-reason-submit-{id}"));
        assert!(view.read_with(visual, |view, _| view.approvals.attempt.is_none()));
        assert_eq!(view.read_with(visual, |_, cx| pending(&view, cx)), Some(id));
        let text = "Read the file first 中文 🙂\nPreserve unrelated changes";
        tap(visual, &format!("live-approval-reason-{id}"));
        visual.simulate_input(text);
        tap(visual, &format!("live-approval-reason-cancel-{id}"));
        assert!(!view.read_with(visual, |view, _| view.approvals.drafts[&id].editing));
        choose(visual, id, "reject", 0);
        assert_eq!(
            view.read_with(visual, |view, cx| view.approvals.drafts[&id]
                .input
                .read(cx)
                .value()),
            text
        );
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(!view.read_with(visual, |view, _| view.approvals.open));
        tap(visual, "live-approval-trigger");
        assert_eq!(
            view.read_with(visual, |view, cx| view.approvals.drafts[&id]
                .input
                .read(cx)
                .value()),
            text
        );
        tap(visual, &format!("live-approval-reason-submit-{id}"));
        wait(visual, |cx| {
            pending(&view, cx).is_some_and(|next| next != id) && !view.read(cx).approvals.pending
        });
        view.read_with(visual, |view, cx| {
            let approval = view.history.calls[0].approval.as_ref().unwrap();
            assert_eq!(approval.state, ApprovalState::Denied);
            assert_eq!(approval.reason.as_deref(), Some(text));
            assert!(approval.choice.is_none());
            assert!(!view.approvals.drafts.contains_key(&id));
            assert_eq!(
                view.input.read(cx).value(),
                "Keep the conversation draft 中文 🙂"
            );
        });
        assert_eq!(
            visual.update(|window, cx| crate::feedback::tests::count(window, "Denied", cx)),
            1
        );
        let next = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        tap(visual, &format!("live-approval-reject-{next}"));
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).approvals.pending
        });
        assert!(!fixture.directory.path().join("project/资料.txt").exists());
        assert!(!fixture.directory.path().join("project/denied.txt").exists());
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn restricts_remembered_choices(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        view.update(visual, |view, cx| {
            Arc::make_mut(&mut view.history.calls)[0]
                .approval
                .as_mut()
                .unwrap()
                .can_remember = false;
            Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page)
                .approvals[0]
                .can_remember = false;
            cx.notify();
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-approve-options-{id}").into_boxed_str()
                ))
                .is_none()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-approve-{id}").into_boxed_str()
                ))
                .is_some()
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.resolve_approval(id, Decision::ApproveSession, window, cx);
                view.resolve_approval(id, Decision::ApprovePrefix("printf %s".into()), window, cx);
                assert!(view.approvals.attempt.is_none());
                Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page)
                    .approvals[0]
                    .can_remember = true;
                view.resolve_approval(id, Decision::ApprovePrefix("printf %s".into()), window, cx);
                assert!(view.approvals.attempt.is_none());
            })
        });
        tap(visual, &format!("live-approval-reject-{id}"));
        wait(visual, |cx| {
            pending(&view, cx).is_some_and(|next| next != id) && !view.read(cx).approvals.pending
        });
        let next = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        tap(visual, &format!("live-approval-reject-{next}"));
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).approvals.pending
        });
        assert!(!fixture.directory.path().join("project/资料.txt").exists());
        assert!(!fixture.directory.path().join("project/denied.txt").exists());
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
