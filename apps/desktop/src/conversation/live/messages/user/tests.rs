use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, open, tap},
    wait,
};
use core::prelude::v1::test;

fn complete(fixture: &Fixture, message: impl Into<Input>) -> TurnId {
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
                    .read_conversation(fixture.session.id, None, 100),
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
        assert!(std::time::Instant::now() < deadline, "completion deadline");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn native_reference_edits_preserve_captured_turn(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        std::fs::write(
            fixture.directory.path().join("project/notes.md"),
            "Fixture notes",
        )
        .unwrap();
        let reference = sailry_protocol::conversation::reference::Reference {
            label: "notes.md".into(),
            target: sailry_protocol::conversation::reference::Target::File("notes.md".into()),
        };
        let turn = complete(
            &fixture,
            Input {
                text: "# Original @notes.md\n".into(),
                attachments: vec![],
                references: vec![reference.clone()],
            },
        );
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.edit_message(turn, window, cx));
            window.draw(cx).clear(cx);
        });
        let original = view.read_with(visual, |view, cx| {
            let editing = view.editing.as_ref().unwrap();
            let Command::ReplaceTurn {
                session,
                expected_revision,
                expected_history_revision,
                ..
            } = &editing.command
            else {
                panic!("replace command expected")
            };
            assert_eq!(*session, fixture.session.id);
            assert_eq!(*expected_revision, fixture.session.revision);
            assert_eq!(
                *expected_history_revision,
                view.history.snapshot.as_ref().unwrap().page.revision
            );
            assert_eq!(editing.input.read(cx).tokens().len(), 1);
            editing.input.read(cx).content()
        });
        visual.simulate_keystrokes("cmd-a backspace cmd-z");
        assert_eq!(
            view.read_with(visual, |view, cx| view
                .editing
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .content()),
            original
        );
        visual.update(|window, cx| {
            let input = view.read(cx).editing.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| {
                input.set_selected_range(0.."# Original".len(), cx);
                input.replace("# Revised", window, cx);
                assert_eq!(input.tokens()[0].token(), original.tokens()[0].token());
            });
        });
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            let view = view.read(cx);
            view.editing.is_none()
                && view.history.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.page.revision == 2
                        && snapshot.page.runs.len() == 1
                        && snapshot.page.runs[0].status == Status::Completed
                })
        });
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            let message = sent_input(page, page.runs[0].turn);
            assert_eq!(message.text, "# Revised @notes.md\n");
            assert_eq!(message.references, [reference]);
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui_kit::test]
fn history_actions(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let first = complete(&fixture, "Original 中文 🙂");
        let last = complete(&fixture, "Later turn");
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        visual.update(|window, _| window.activate_window());
        wait(visual, |cx| view.read(cx).connected());
        view.update(visual, |view, cx| {
            view.scroller
                .update(cx, |state, cx| state.scroll_to_item(0, cx))
        });
        tap(visual, &format!("live-user-copy-{first}"));
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "Original 中文 🙂"
        );
        tap(visual, &format!("live-user-edit-{first}"));
        assert!(view.read_with(visual, |view, cx| {
            view.editing.as_ref().unwrap().input.read(cx).value() == "Original 中文 🙂"
        }));
        visual.simulate_keystrokes("escape");
        assert!(view.read_with(visual, |view, _| view.editing.is_none()));
        assert_eq!(
            view.read_with(visual, |view, _| view.rows.clone()),
            [first, last]
        );
        tap(visual, &format!("live-user-edit-{first}"));
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("Discard this change");
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        visual.update(|window, cx| {
            view.read(cx)
                .input
                .clone()
                .update(cx, |input, cx| input.focus(window, cx));
        });
        wait(visual, |cx| view.read(cx).editing.is_none());
        assert_eq!(
            view.read_with(visual, |view, _| view.rows.clone()),
            [first, last]
        );
        tap(visual, &format!("live-user-edit-{first}"));
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.input
                    .update(cx, |input, cx| input.set_value("Unsent draft", window, cx));
                view.editing
                    .as_ref()
                    .unwrap()
                    .input
                    .update(cx, |input, cx| {
                        input.set_value("Revised 中文 🙂", window, cx)
                    });
            })
        });
        visual.simulate_keystrokes("cmd-end shift-enter");
        assert!(view.read_with(visual, |view, cx| {
            view.editing
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .value()
                .contains('\n')
        }));
        assert_eq!(
            view.read_with(visual, |view, _| view.rows.clone()),
            [first, last]
        );
        visual.simulate_keystrokes("backspace");
        assert_eq!(
            view.read_with(visual, |view, cx| view
                .editing
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .value()
                .to_string()),
            "Revised 中文 🙂"
        );
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            let view = view.read(cx);
            view.editing.is_none()
                && view.history.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.page.runs.len() == 1
                        && snapshot.page.runs[0].status == Status::Completed
                })
        });
        let backup = view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "Unsent draft");
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(page.revision, 2);
            assert!(
                page.entries.iter().any(|entry| entry.author == "user"
                    && entry.parts.contains(&Part::Text("Revised 中文 🙂".into()))),
                "{:?}",
                page.entries
            );
            view.backup.as_ref().unwrap().id
        });
        let original = fixture
            .runtime
            .block_on(fixture.binding.client.read_conversation(backup, None, 100))
            .unwrap()
            .page;
        assert_eq!(
            original.runs.iter().map(|run| run.turn).collect::<Vec<_>>(),
            [first, last]
        );
        fixture.close();
    }
}

#[gpui_kit::test]
fn idempotent_replacement(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let first = complete(&fixture, "Retained");
        let last = complete(&fixture, "Replace me");
        let client = &fixture.binding.client;
        let before = fixture
            .runtime
            .block_on(client.read_conversation(fixture.session.id, None, 100))
            .unwrap()
            .page;
        let command = Command::ReplaceTurn {
            session: fixture.session.id,
            turn: last,
            expected_head: last,
            expected_history_revision: before.revision,
            expected_revision: 1,
            message: Input {
                text: "Replacement".into(),
                attachments: vec![sailry_protocol::AttachmentId::new()],
                references: vec![],
            },
        };
        assert!(
            fixture
                .runtime
                .block_on(client.execute(client.prepare(command.clone())))
                .is_err()
        );
        assert_eq!(
            fixture
                .runtime
                .block_on(client.read_conversation(fixture.session.id, None, 100))
                .unwrap()
                .page,
            before
        );
        let mut command = command;
        let Command::ReplaceTurn { message, .. } = &mut command else {
            unreachable!()
        };
        message.attachments.clear();
        let request = client.prepare(command);
        let output = fixture
            .runtime
            .block_on(client.execute(request.clone()))
            .unwrap();
        assert_eq!(
            fixture.runtime.block_on(client.execute(request)).unwrap(),
            output
        );
        let Output::TurnReplaced { turn, history } = output else {
            panic!("replacement expected")
        };
        assert_eq!(history.through, Some(first));
        let page = fixture
            .runtime
            .block_on(client.read_conversation(fixture.session.id, None, 100))
            .unwrap()
            .page;
        assert!(page.runs.iter().any(|run| run.turn == first));
        assert!(!page.runs.iter().any(|run| run.turn == last));
        assert!(
            page.runs.iter().any(|run| run.turn == turn.id)
                || page.queue.items.iter().any(|item| item.turn == turn.id)
        );
        fixture.close();
    }
}

#[gpui_kit::test]
fn preserves_literal_text(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let text =
            "https://example.com，等待加载后读取页面。\n\n**literal** [label](https://example.org)";
        let turn = complete(&fixture, text);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let links = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured = links.clone();
        let _events = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::Link(link) = event {
                    captured.borrow_mut().push(link.clone());
                }
            })
        });
        tap(visual, &format!("live-user-text-{turn}"));
        assert!(links.borrow().is_empty());
        assert!(visual.opened_url().is_none());
        view.read_with(visual, |view, _| {
            assert!(!view.texts.borrow().contains_key(&format!("{turn}-prompt")))
        });
        tap(visual, &format!("live-user-copy-{turn}"));
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            text
        );
        tap(visual, &format!("live-user-edit-{turn}"));
        assert!(view.read_with(visual, |view, cx| {
            view.editing.as_ref().unwrap().input.read(cx).value() == text
        }));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
