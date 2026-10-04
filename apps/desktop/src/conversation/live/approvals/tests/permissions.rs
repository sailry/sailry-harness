use super::*;
use sailry_protocol::{Permission, conversation::ApprovalSource};

fn select(view: &Entity<View>, visual: &mut VisualTestContext, mode: Permission) {
    tap(visual, "live-chat-permission");
    tap(
        visual,
        &format!("{}-option", crate::conversation::permission::label(mode)),
    );
    wait(visual, |cx| {
        let view = view.read(cx);
        !view.pending && view.config.as_ref().unwrap().permission == mode
    });
}

#[gpui::test]
fn preserves_pending_turns_and_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let first = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        tap(visual, "live-chat-input");
        visual.simulate_input("next draft 中文");
        tap(visual, "live-chat-permission");
        visual.simulate_keystrokes("escape");
        visual.simulate_input(" preserved");
        select(&view, visual, Permission::Full);
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "next draft 中文 preserved");
            assert_eq!(view.session.as_ref().unwrap().revision, 2);
            assert_eq!(
                view.history.calls[0].approval.as_ref().unwrap().source,
                ApprovalSource::User
            );
        });
        assert_eq!(
            view.read_with(visual, |_, cx| pending(&view, cx)),
            Some(first)
        );
        assert!(!fixture.directory.path().join("project/资料.txt").exists());
        tap(visual, "live-approval-trigger");
        tap(visual, &format!("live-approval-approve-{first}"));
        wait(visual, |cx| {
            pending(&view, cx).is_some_and(|next| next != first)
        });
        let second = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        assert!(!fixture.directory.path().join("project/denied.txt").exists());
        tap(visual, &format!("live-approval-reject-{second}"));
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).approvals.pending
        });
        let session = view.read_with(visual, |view, _| view.session.clone().unwrap());
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = open(cx, fixture.binding.clone(), session);
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().permission),
            Permission::Full
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[cfg(unix)]
#[gpui::test]
fn applies_project_and_full(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for mode in [Permission::Project, Permission::Full] {
            let fixture = Fixture::with_tools(
                remote,
                vec![
                    (
                        crate::agent_fixture::plugin_tool("files", "write_file"),
                        json!({"path": "自动.txt", "text": "中文 🙂", "expected_revision": null}),
                    ),
                    (
                        crate::agent_fixture::plugin_tool("commands", "run_command"),
                        json!({"command": "printf x >> counter.txt"}),
                    ),
                ],
            );
            let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| view.read(cx).connected());
            select(&view, visual, mode);
            tap(visual, "live-chat-input");
            visual.simulate_input("apply selected permission");
            visual.simulate_keystrokes("enter");
            let root = fixture.directory.path().join("project");
            if mode == Permission::Project {
                wait(visual, |cx| pending(&view, cx).is_some());
                assert_eq!(
                    std::fs::read_to_string(root.join("自动.txt")).unwrap(),
                    "中文 🙂"
                );
                assert!(!root.join("counter.txt").exists());
                let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
                tap(visual, &format!("live-approval-approve-{id}"));
            }
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
                let sources: Vec<_> = view
                    .history
                    .calls
                    .iter()
                    .map(|call| call.approval.as_ref().unwrap().source)
                    .collect();
                assert_eq!(
                    sources,
                    if mode == Permission::Project {
                        vec![ApprovalSource::Project, ApprovalSource::User]
                    } else {
                        vec![ApprovalSource::Full, ApprovalSource::Full]
                    }
                );
                assert_eq!(
                    view.history.snapshot.as_ref().unwrap().page.runs[0].revision,
                    2
                );
            });
            assert_eq!(
                std::fs::read_to_string(root.join("counter.txt")).unwrap(),
                "x"
            );
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn merges_with_current_configuration(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-permission");
        let mut config = fixture.session.config.clone();
        config.effort = Effort::Low;
        fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 1,
            config,
        });
        wait(visual, |cx| {
            view.read(cx).session.as_ref().unwrap().revision == 2
        });
        tap(visual, "composer_permission_project-option");
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).session.as_ref().unwrap().revision == 3
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().effort),
            Effort::Low
        );

        tap(visual, "live-chat-effort");
        let mut config = view.read_with(visual, |view, _| view.config.clone().unwrap());
        config.permission = Permission::Full;
        fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 3,
            config,
        });
        wait(visual, |cx| {
            view.read(cx).session.as_ref().unwrap().revision == 4
        });
        visual.simulate_keystrokes("end");
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).session.as_ref().unwrap().revision == 5
        });
        view.read_with(visual, |view, _| {
            let config = view.config.as_ref().unwrap();
            assert_eq!(config.permission, Permission::Full);
            assert_eq!(config.effort, Effort::High);
        });
        tap(visual, "live-chat-permission");
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.node.connected = false;
                cx.notify();
            })
        });
        tap(visual, "composer_permission_ask-option");
        assert_eq!(
            view.read_with(visual, |view, _| view.session.as_ref().unwrap().revision),
            5
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
