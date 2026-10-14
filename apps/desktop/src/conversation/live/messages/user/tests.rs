use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, open, tap},
    wait,
};
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt as _;

#[derive(Default)]
struct Surfaces(BTreeMap<(WindowId, TurnId), Bounds<Pixels>>);

impl Global for Surfaces {}

pub(in crate::conversation::live::messages) fn observe(
    turn: TurnId,
    bubble: Bubble,
) -> impl IntoElement {
    Surface {
        turn,
        bubble: bubble.into_any_element(),
    }
}

struct Surface {
    turn: TurnId,
    bubble: AnyElement,
}

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Surface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // Share the actual Bubble's layout node: observation must not change
        // its constraints or inspect the border-inset content rectangle.
        (self.bubble.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if !cx.has_global::<Surfaces>() {
            cx.set_global(Surfaces::default());
        }
        cx.global_mut::<Surfaces>()
            .0
            .insert((window.window_handle().window_id(), self.turn), bounds);
        self.bubble.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.bubble.paint(window, cx);
    }
}

#[gpui_kit::test]
fn adapts_bubble_width(cx: &mut TestAppContext) {
    use sailry_protocol::conversation::reference::{Reference, Target};
    init(cx);
    let fixture = Fixture::with_tools(false, vec![]);
    std::fs::write(fixture.directory.path().join("project/notes.md"), "Notes").unwrap();
    let plain = complete(&fixture, "Hi");
    let reference = Reference {
        label: "notes.md".into(),
        target: Target::File("notes.md".into()),
    };
    let short = complete(
        &fixture,
        Input {
            text: "See @notes.md".into(),
            attachments: vec![],
            references: vec![reference.clone()],
        },
    );
    let long = complete(
        &fixture,
        Input {
            text: format!(
                "{} @notes.md",
                "A longer message with Unicode 内容. ".repeat(12)
            ),
            attachments: vec![],
            references: vec![reference],
        },
    );
    let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(960.), px(1100.)));
    wait(visual, |cx| view.read(cx).connected());
    view.update(visual, |view, cx| {
        view.scroller
            .update(cx, |state, cx| state.scroll_to_item(0, cx))
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut previous = None;
    let bounds = loop {
        visual.run_until_parked();
        let current = visual.update(|window, cx| {
            window.render_frame(cx);
            [plain, short, long].map(|turn| {
                cx.try_global::<Surfaces>().and_then(|surfaces| {
                    surfaces
                        .0
                        .get(&(window.window_handle().window_id(), turn))
                        .copied()
                })
            })
        });
        let measured = current[1].is_some_and(|bounds| {
            bounds.size.width < px(crate::conversation::layout::USER_MESSAGE_WIDTH)
        });
        if measured
            && previous == Some(current)
            && let [Some(plain), Some(short), Some(long)] = current
        {
            break [plain, short, long];
        }
        assert!(
            std::time::Instant::now() < deadline,
            "adaptive bubble layout deadline: {current:?}"
        );
        previous = Some(current);
    };
    let [plain, short, long] = bounds;
    let maximum = px(crate::conversation::layout::USER_MESSAGE_WIDTH);
    assert!(
        plain.size.width > px(0.) && plain.size.width < maximum,
        "{bounds:?}"
    );
    assert!(
        short.size.width > plain.size.width && short.size.width < maximum,
        "{bounds:?}"
    );
    assert!((long.size.width - maximum).abs() <= px(1.), "{bounds:?}");
    assert!(
        long.size.height > short.size.height,
        "long prompt must wrap: {bounds:?}"
    );
    for bounds in [plain, short] {
        assert!(
            (bounds.right() - long.right()).abs() <= px(1.),
            "bubbles retain trailing alignment: {bounds:?}, long={long:?}"
        );
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

#[gpui_kit::test]
fn hides_automatic_input_and_marker(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let turn = complete(&fixture, "Private continuation instructions");
        let package = fixture.files_context().package;
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        visual.update(|window, cx| window.render_frame(cx));
        let prompt_id = view.read_with(visual, |view, _| {
            view.prompts.borrow().get(&turn).unwrap().entity_id()
        });
        visual.update(|window, _| assert!(window.try_find(("prompt", prompt_id)).is_some()));
        let mut history = view.read_with(visual, |view, _| view.history.clone());
        let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
        page.runs[0].continuation = Some(sailry_protocol::conversation::Continuation {
            package,
            marker: Some(sailry_protocol::plugin::desktop::Navigation {
                label: "Continuing task".into(),
                locales: BTreeMap::new(),
                icon: None,
            }),
        });
        let original = page.entries.clone();
        let continuation = page.runs[0].continuation.clone();
        view.update(visual, |view, cx| view.update_history(history.clone(), cx));
        let marker: &'static str = Box::leak(format!("live-continuation-{turn}").into_boxed_str());
        visual.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(("prompt", prompt_id)).is_none());
        });
        assert!(visual.debug_bounds(marker).is_none());
        assert_eq!(
            view.read_with(visual, |view, _| view
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .entries
                .clone()),
            original
        );
        assert_eq!(
            view.read_with(visual, |view, _| view
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .runs[0]
                .continuation
                .clone()),
            continuation
        );
        let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
        page.runs[0].status = Status::Continued;
        view.update(visual, |view, cx| view.update_history(history.clone(), cx));
        assert!(view.read_with(visual, |view, _| view.rows.contains(&turn)));
        let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
        page.entries.retain(|entry| entry.author == "user");
        view.update(visual, |view, cx| view.update_history(history, cx));
        assert!(view.read_with(visual, |view, _| view.rows.is_empty()));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

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
        let backup = history
            .backup
            .as_ref()
            .expect("replacement backup expected");
        assert_eq!(
            fixture
                .runtime
                .block_on(client.read_conversation(backup.id, None, 100))
                .unwrap()
                .page
                .entries,
            before.entries
        );
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

#[gpui_kit::test]
fn themed_actions(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::styled(directory.path(), "actions");
    let loaded = crate::theme::package::Loaded::read(&source, true).unwrap();
    cx.update(|cx| {
        cx.global_mut::<crate::theme::Catalog>()
            .packages
            .push(loaded.into())
    });
    let fixture = Fixture::with_tools(false, vec![]);
    let turn = complete(&fixture, "Keep this message");
    let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
    wait(visual, |cx| view.read(cx).connected());
    let history = view.read_with(visual, |view, _| view.history.snapshot.clone().unwrap());
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            crate::theme::apply(1, Some(window), cx);
            Theme::change(mode, Some(window), cx);
            view.update(cx, |_, cx| cx.notify());
        });
        wait(visual, |_| true);
        assert!(visual.debug_bounds("theme-message.user.top_left").is_some());
        tap(visual, &format!("live-user-copy-{turn}"));
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "Keep this message"
        );
        tap(visual, &format!("live-user-edit-{turn}"));
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("Retained edit");
        visual.update(|window, cx| {
            crate::theme::apply(0, Some(window), cx);
            view.update(cx, |_, cx| cx.notify());
        });
        wait(visual, |_| true);
        assert!(visual.debug_bounds("theme-message.user.top_left").is_none());
        assert!(view.read_with(visual, |view, cx| {
            Arc::ptr_eq(view.history.snapshot.as_ref().unwrap(), &history)
                && view.editing.as_ref().unwrap().input.read(cx).value() == "Retained edit"
        }));
        visual.simulate_keystrokes("escape");
        assert!(view.read_with(visual, |view, _| view.editing.is_none()));
    }
    assert_eq!(fixture.task_requests(), 1);
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
