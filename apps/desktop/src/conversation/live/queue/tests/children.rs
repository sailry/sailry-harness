use super::*;
use crate::conversation::live::subagents::fixture::Fixture as ChildFixture;

#[gpui::test]
fn retained_input_is_read_only(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = ChildFixture::with_children(remote, 1);
        fixture.start();
        let binding = Binding {
            client: fixture.client.clone(),
            defaults: fixture.client.clone(),
            runtime: fixture.runtime.clone(),
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            host: "Fixture".into(),
            project_name: "Child history".into(),
            branch: "main".into(),
        };
        let (parent, visual) = crate::conversation::live::tests::fixture::open(
            cx,
            binding.clone(),
            fixture.session.clone(),
        );
        wait(visual, |cx| {
            parent
                .read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.page.children.first().is_some_and(|child| {
                        parent.read(cx).child_session(child.run.session).is_some()
                            && !fixture.page(child.run.session).approvals.is_empty()
                    })
                })
        });
        let session = parent.read_with(visual, |parent, _| {
            parent
                .child_session(
                    parent.history.snapshot.as_ref().unwrap().page.children[0]
                        .run
                        .session,
                )
                .unwrap()
        });
        let recipient = fixture.page(session.id).runs[0].clone();
        std::fs::write(
            fixture.directory.path().join("project/context.txt"),
            "Reference context",
        )
        .unwrap();
        let message = sailry_protocol::conversation::Input {
            text: "Preserve internal guidance 中文 🙂 @context.txt".into(),
            attachments: vec![],
            references: vec![sailry_protocol::conversation::reference::Reference {
                label: "context.txt".into(),
                target: sailry_protocol::conversation::reference::Target::File(
                    "context.txt".into(),
                ),
            }],
        };
        // Internal admission remains available without a controller input box.
        let Output::QueuedTurn(source) = fixture.execute(Command::GuideTurn {
            recipient: recipient.turn,
            expected_revision: recipient.revision,
            message: message.clone(),
        }) else {
            panic!("guidance expected")
        };
        fixture.execute(Command::StopTurn {
            turn: fixture.page(fixture.session.id).runs[0].turn,
        });
        wait(visual, |_| {
            fixture.page(session.id).queue.paused
                && fixture
                    .page(session.id)
                    .runs
                    .iter()
                    .all(|run| !matches!(run.status, Status::Running | Status::Stopping))
        });
        visual.update(|window, _| window.remove_window());
        drop(parent);
        let (view, visual) = crate::conversation::live::tests::fixture::open(cx, binding, session);
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected && panel.read(cx).queue.items.len() == 1
        });
        for selector in [
            "child-guide-input",
            "child-guide",
            "child-guide-attach",
            "live-chat-input",
            "live-chat-send",
        ] {
            assert!(visual.debug_bounds(selector).is_none());
        }
        click(visual, "live-queue");
        assert!(visual.debug_bounds(selector("read", source.id)).is_some());
        for action in ["edit", "send", "guide", "drag"] {
            assert!(visual.debug_bounds(selector(action, source.id)).is_none());
        }
        assert!(visual.debug_bounds("queue-resume").is_none());
        click(visual, selector("read", source.id));
        wait(visual, |cx| panel.read(cx).editing.is_some());
        assert!(visual.debug_bounds("queue-edit-save").is_none());
        let original = panel.read_with(visual, |panel, cx| {
            let editing = panel.editing.as_ref().unwrap();
            assert_eq!(editing.references, message.references);
            let content = editing.input.read(cx).content();
            assert_eq!(content.tokens().len(), 1);
            content
        });
        click(visual, "queue-input");
        visual.simulate_keystrokes("secondary-a backspace enter");
        visual.simulate_input("Must not edit");
        assert_eq!(
            panel.read_with(visual, |panel, cx| panel
                .editing
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .content()),
            original
        );
        click(visual, "queue-read-copy");
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            message.text
        );
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.save(window, cx);
                panel.execute(
                    Command::SendQueuedTurn {
                        turn: source.id,
                        expected_revision: 1,
                    },
                    window,
                    cx,
                );
                assert!(!panel.pending && panel.retry.is_none());
            })
        });
        click(visual, selector("delete", source.id));
        wait(visual, |cx| {
            panel.read(cx).queue.items.is_empty() && panel.read(cx).editing.is_none()
        });
        assert!(fixture.page(source.session).queue.items.is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
