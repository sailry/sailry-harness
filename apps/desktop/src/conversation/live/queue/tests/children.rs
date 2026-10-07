use super::*;
use crate::conversation::live::subagents::fixture::Fixture as ChildFixture;
use crate::conversation::live::tests::fixture::tap;

fn open_child<'a>(
    cx: &'a mut TestAppContext,
    fixture: &ChildFixture,
) -> (Entity<View>, &'a mut VisualTestContext) {
    fixture.start();
    let binding = Binding {
        client: fixture.client.clone(),
        defaults: fixture.client.clone(),
        runtime: fixture.runtime.clone(),
        project: fixture.session.project,
        worktree: Some(fixture.session.worktree),
        host: "Fixture".into(),
        project_name: "Child guidance".into(),
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
        let id = parent.history.snapshot.as_ref().unwrap().page.children[0]
            .run
            .session;
        parent.child_session(id).unwrap()
    });
    visual.update(|window, _| window.remove_window());
    drop(parent);
    let mut binding = binding;
    binding.worktree = Some(session.worktree);
    let (view, visual) = crate::conversation::live::tests::fixture::open(cx, binding, session);
    wait(visual, |cx| {
        view.read(cx).connected() && view.read(cx).guide_target().is_some()
    });
    (view, visual)
}

#[gpui::test]
fn retained_input_is_read_only(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = ChildFixture::with_children(remote, 1);
        let path = fixture.directory.path().join("guidance.txt");
        std::fs::write(&path, "Complete guidance attachment 中文 🙂").unwrap();
        std::fs::write(
            fixture.directory.path().join("project/context.txt"),
            "Reference context",
        )
        .unwrap();
        let (view, visual) = open_child(cx, &fixture);
        let recipient = view.read_with(visual, |view, _| view.guide_target().unwrap());
        let permit = view.read_with(visual, |view, _| {
            view.attachments
                .slots
                .clone()
                .try_acquire_many_owned(2)
                .unwrap()
        });
        assert!(visual.debug_bounds("child-guide-input").is_some());
        assert!(visual.debug_bounds("live-chat-input").is_none());
        assert!(visual.debug_bounds("live-chat-send").is_none());
        assert!(visual.debug_bounds("live-chat-model").is_none());
        assert!(visual.debug_bounds("live-chat-mode").is_none());
        tap(visual, "child-guide-input");
        visual.simulate_input("/plan\nPreserve all guidance 中文 🙂 ");
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.insert_file_references(vec![("context.txt".into(), false)], window, cx);
                view.restore_attachment_sources(
                    vec![crate::content::images::Local::Path(path.clone())],
                    cx,
                );
            })
        });
        let message = view.read_with(visual, |view, cx| sailry_protocol::conversation::Input {
            text: view.input.read(cx).value().to_string(),
            attachments: vec![],
            references: view.active_references(cx),
        });
        tap(visual, "child-guide");
        view.read_with(visual, |view, _| {
            assert_eq!(view.prepared_guidance, Some(recipient));
            assert!(view.preparing.is_some());
        });
        // An upload completion must use the revision captured by the click,
        // not a newer controller snapshot observed while staging was pending.
        view.update(visual, |view, cx| {
            let page =
                Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page);
            page.runs
                .iter_mut()
                .find(|run| run.turn == recipient.turn)
                .unwrap()
                .revision += 100;
            cx.notify();
        });
        drop(permit);
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).queue.items.len() == 1 && !view.read(cx).busy()
        });
        let source = panel.read_with(visual, |panel, _| {
            assert!(panel.child);
            assert_eq!(panel.queue.items[0].guidance, Some(recipient.turn));
            panel.queue.items[0].turn
        });
        let Output::QueuedMessage(queued) =
            fixture.execute(Command::ReadQueuedTurn { turn: source })
        else {
            panic!("queued input expected")
        };
        assert_eq!(queued.message.text, message.text);
        assert_eq!(queued.message.references, message.references);
        assert_eq!(queued.message.attachments.len(), 1);
        let attachment = panel.read_with(visual, |panel, _| {
            panel.queue.items[0].attachments[0].clone()
        });
        assert_eq!(queued.message.attachments, [attachment.id]);
        let Output::AttachmentDownload(download) = fixture.execute(Command::DownloadAttachment {
            worktree: attachment.spec.worktree,
            attachment: attachment.id,
        }) else {
            panic!("attachment download expected")
        };
        let mut bytes = Vec::new();
        fixture
            .runtime
            .block_on(fixture.client.download_attachment(
                &download,
                &mut bytes,
                CancellationToken::new(),
                |_| {},
            ))
            .unwrap();
        assert_eq!(bytes, std::fs::read(&path).unwrap());
        view.read_with(visual, |view, cx| {
            assert!(view.input.read(cx).value().is_empty());
            assert_eq!(view.current_mode(), sailry_protocol::WorkMode::Code);
        });
        let parent = fixture.page(fixture.session.id).runs[0].turn;
        fixture.execute(Command::StopTurn { turn: parent });
        wait(visual, |cx| {
            panel.read(cx).queue.paused
                && panel
                    .read(cx)
                    .queue
                    .items
                    .first()
                    .is_some_and(|item| item.guidance.is_none())
                && view.read(cx).active().is_none()
        });
        assert!(visual.debug_bounds("child-guide-input").is_none());
        assert!(view.read_with(visual, |view, _| view.outgoing.is_none()));
        view.update(visual, |view, cx| {
            view.show_outgoing(queued.message.clone(), cx);
            view.outgoing.as_mut().unwrap().turn = Some(TurnId::new());
            view.reconcile_outgoing();
            assert!(
                view.outgoing.is_some(),
                "another turn must not consume the draft"
            );
            view.outgoing.as_mut().unwrap().turn = Some(source);
            view.reconcile_outgoing();
            assert!(
                view.outgoing.is_none(),
                "the exact retained source is admitted"
            );
            view.sync_rows(cx);
            cx.notify();
        });
        click(visual, "live-queue");
        assert!(visual.debug_bounds(selector("read", source)).is_some());
        assert!(visual.debug_bounds(selector("edit", source)).is_none());
        assert!(visual.debug_bounds(selector("send", source)).is_none());
        assert!(visual.debug_bounds(selector("guide", source)).is_none());
        assert!(visual.debug_bounds(selector("drag", source)).is_none());
        click(visual, selector("read", source));
        wait(visual, |cx| panel.read(cx).editing.is_some());
        assert!(visual.debug_bounds("queue-edit-save").is_none());
        assert!(!panel.read_with(visual, |panel, _| panel.editing_draft()));
        let original = panel.read_with(visual, |panel, cx| {
            let editing = panel.editing.as_ref().unwrap();
            assert_eq!(editing.references, message.references);
            assert_eq!(editing.attachments, queued.message.attachments);
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
                        turn: source,
                        expected_revision: queued.revision,
                    },
                    window,
                    cx,
                );
                assert!(!panel.pending && panel.retry.is_none());
            })
        });
        click(visual, selector("delete", source));
        wait(visual, |cx| {
            panel.read(cx).queue.items.is_empty() && panel.read(cx).editing.is_none()
        });
        assert!(fixture.page(queued.turn.session).queue.items.is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn keeps_rejected_guidance_draft(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = ChildFixture::with_children(remote, 1);
        let path = fixture.directory.path().join("draft.txt");
        std::fs::write(&path, "Retained attachment bytes").unwrap();
        let (view, visual) = open_child(cx, &fixture);
        let permit = view.read_with(visual, |view, _| {
            view.attachments
                .slots
                .clone()
                .try_acquire_many_owned(2)
                .unwrap()
        });
        tap(visual, "child-guide-input");
        visual.simulate_input("Keep this unaccepted guidance 中文 🙂");
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.restore_attachment_sources(
                    vec![crate::content::images::Local::Path(path.clone())],
                    cx,
                )
            })
        });
        let draft = view.read_with(visual, |view, cx| view.input.read(cx).value());
        tap(visual, "child-guide");
        fixture.execute(Command::StopTurn {
            turn: fixture.page(fixture.session.id).runs[0].turn,
        });
        wait(visual, |cx| view.read(cx).active().is_none());
        drop(permit);
        wait(visual, |cx| {
            !view.read(cx).busy() && view.read(cx).error.is_some()
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), draft);
            assert_eq!(view.attachments.ids().len(), 1);
            assert!(view.prepared_guidance.is_none());
            let snapshot = view.snapshot_draft(cx).unwrap().unwrap();
            assert_eq!(snapshot.draft.text, draft.as_ref());
            assert_eq!(snapshot.sources.len(), 1);
            assert!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .queue
                    .items
                    .is_empty()
            );
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
