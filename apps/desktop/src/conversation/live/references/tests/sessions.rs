use super::*;
use crate::shell::session_scope::Key;

#[gpui::test]
fn loads_unopened_sessions_on_the_captured_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(false, vec![]);
        let desktop = mounted::desktop(&fixture);
        fixture.start();
        let history = mounted::finished(&fixture);
        let Output::Session(source) = fixture.execute(Command::CreateSession {
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            config: Some(fixture.session.config.clone()),
        }) else {
            panic!("source session expected")
        };
        let (shell, view, visual) = mounted::open(&fixture, &desktop, remote, source.clone(), cx);
        assert!(!shell.read_with(visual, |shell, _| {
            shell
                .chats
                .views
                .contains_key(&(fixture.node.id(), fixture.session.id))
        }));
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let token = view.remember_reference(Reference {
                    target: Target::Session(fixture.session.id),
                    label: "Target".into(),
                });
                view.input.update(cx, |input, cx| {
                    input.set_value("Keep draft ", window, cx);
                    let end = input.value().len();
                    input.set_selected_range(end..end, cx);
                    input.replace_with_token(token, window, cx).unwrap();
                });
            });
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(desktop.id(), cx)
            });
        });
        inline::click_token(visual, &view, "@Target");
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                let chat = chat.read(cx);
                chat.session() == Some(fixture.session.id)
                    && chat.binding.client.target() == fixture.node.id()
                    && chat
                        .history
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.page.entries == history.page.entries)
            })
        });
        let loaded = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx)),
            "Keep draft @Target"
        );
        assert_eq!(
            fixture.task_requests(),
            1,
            "opening a reference must not start a turn"
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.activate_session(Key::Session(fixture.node.id(), source.id), window, cx)
            })
        });
        inline::click_token(visual, &view, "@Target");
        wait(visual, |cx| shell.read(cx).current_chat() == Some(&loaded));
        let before = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.activate_session(Key::Session(fixture.node.id(), source.id), window, cx)
            });
            window.clear_notifications(cx);
            let before = crate::feedback::tests::count(window, &tr("reference_unavailable"), cx);
            view.update(cx, |_, cx| cx.emit(Event::Session(SessionId::new())));
            before
        });
        unavailable(visual, before);
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone()),
            view
        );
        let Output::Session(target) = fixture.execute(Command::ReadSession {
            session: fixture.session.id,
        }) else {
            panic!("target session expected")
        };
        fixture.execute(Command::RemoveSession {
            session: target.id,
            expected_revision: target.revision,
        });
        let before = visual.update(|window, cx| {
            window.clear_notifications(cx);
            let before = crate::feedback::tests::count(window, &tr("reference_unavailable"), cx);
            // Exercise a stale click even while an old target view remains cached.
            view.update(cx, |_, cx| cx.emit(Event::Session(target.id)));
            before
        });
        unavailable(visual, before);
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone()),
            view
        );
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx)),
            "Keep draft @Target"
        );
        assert_eq!(fixture.task_requests(), 1);
        visual.update(|window, _| window.remove_window());
        fixture.runtime.block_on(desktop.shutdown()).unwrap();
        fixture.close();
    }
}

fn unavailable(visual: &mut VisualTestContext, before: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        if visual.update(|window, cx| {
            !window.notifications(cx).is_empty()
                && crate::feedback::tests::count(window, &tr("reference_unavailable"), cx) > before
        }) {
            return;
        }
        // A cached deleted conversation can report its own failed background read
        // before the reference's authoritative Snapshot request finishes.
        assert!(
            std::time::Instant::now() < deadline,
            "reference failure toast deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
