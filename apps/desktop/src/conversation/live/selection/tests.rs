use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, open, tap},
    wait,
};
use core::prelude::v1::test;
use sailry_protocol::conversation::{Input, Part, Status};

fn complete(fixture: &Fixture, text: &str) -> TurnId {
    let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: 1,
        message: Input::from(text),
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

fn bounds(visual: &mut VisualTestContext, selector: String) -> Bounds<Pixels> {
    visual
        .debug_bounds(Box::leak(selector.into_boxed_str()))
        .unwrap()
}

fn toggle(view: &Entity<View>, visual: &mut VisualTestContext, turn: TurnId) {
    use gpui_kit::test::TestWindowExt;

    let state = view.read_with(visual, |view, _| view.prompts.borrow()[&turn].entity_id());
    let id: ElementId = ("prompt-expand", state).into();
    // A wrapped bubble can move its toggle beneath the composer after expanding.
    // Scroll the real list before clicking the Kit control's current bounds.
    for _ in 0..4 {
        let button = visual.update(|window, cx| {
            window.render_frame(cx);
            window.find(id.clone())
        });
        let viewport = visual.debug_bounds("live-history-viewport").unwrap();
        let center = button.bounds().center();
        if button.visible()
            && center.y >= viewport.top() + px(24.)
            && center.y <= viewport.bottom() - px(48.)
        {
            break;
        }
        visual.simulate_event(ScrollWheelEvent {
            position: viewport.center(),
            delta: ScrollDelta::Pixels(point(px(0.), viewport.center().y - center.y)),
            touch_phase: TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        wait(visual, |_| true);
    }
    visual.update(|window, cx| {
        window.render_frame(cx);
        let button = window.find(id.clone());
        assert!(
            button.visible(),
            "prompt toggle must be inside the viewport"
        );
        assert_ne!(button.disabled(), Some(true));
        window.click(id, cx);
    });
    wait(visual, |_| true);
}

#[gpui::test]
fn folds_wrapped_prompts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let short = complete(&fixture, "Short 中文 🙂");
        let text = "A wrapping message with 中文 and 🙂 that stays intact. ".repeat(35);
        let turn = complete(&fixture, &text);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).rows.contains(&turn));
        for (width, size) in [(420., 12), (960., 18)] {
            let handle = visual.update(|window, cx| {
                crate::preferences::update(cx, |data| data.message_text_size = Some(size));
                window.window_handle()
            });
            visual.simulate_window_resize(handle, gpui_kit::size(px(width), px(1100.)));
            wait(visual, |cx| {
                view.read(cx).prompts.borrow().contains_key(&turn)
            });
            for _ in 0..3 {
                wait(visual, |_| true);
            }
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("message-expand-{short}").into_boxed_str()
                    ))
                    .is_none()
            );
            let collapsed = bounds(visual, format!("message-preview-{turn}"));
            assert!((collapsed.size.height - px(f32::from(size) * 1.6 * 5.)).abs() <= px(1.));
            let control = bounds(visual, format!("message-expand-{turn}"));
            assert!((control.center().x - collapsed.center().x).abs() <= px(1.));
            assert!(control.top() >= collapsed.bottom());
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("message-expand-background-{turn}").into_boxed_str()
                    ))
                    .is_none()
            );
            assert!(visual.debug_bounds("message-preview-fade").is_none());
            toggle(&view, visual, turn);
            let expanded = bounds(visual, format!("message-preview-{turn}"));
            assert!(expanded.size.height > collapsed.size.height);
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("message-expand-background-{turn}").into_boxed_str()
                    ))
                    .is_none()
            );
            assert_eq!(
                expanded.size.height,
                bounds(visual, format!("message-text-{turn}")).size.height
            );
            tap(visual, &format!("live-user-copy-{turn}"));
            assert_eq!(
                visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
                text
            );
            toggle(&view, visual, turn);
            assert_eq!(
                bounds(visual, format!("message-preview-{turn}"))
                    .size
                    .height,
                collapsed.size.height
            );
        }
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
        assert_eq!(page.runs.len(), 2);
        assert!(
            page.entries
                .iter()
                .any(|entry| entry.turn == turn && entry.parts.contains(&Part::Text(text.clone())))
        );
        fixture.close();
    }
}

fn drag(visual: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    wait(visual, |_| true);
    visual.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    wait(visual, |_| true);
    visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    for _ in 0..3 {
        visual.update(|window, cx| window.simulate_next_frame(cx));
        wait(visual, |_| true);
    }
}

#[gpui::test]
fn excerpt_actions(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let text = "Selected 中文 🙂 excerpt and unselected ending";
        let turn = complete(&fixture, text);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).rows.contains(&turn));
        let mut new = None;
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        visual.update(|_, cx| {
            let captured = events.clone();
            cx.subscribe(&view, move |_, event, _| {
                if let Event::NewConversation(text) = event {
                    captured.borrow_mut().push(text.clone());
                }
            })
            .detach();
        });
        for action in ["copy", "quote", "new"] {
            let text_bounds = bounds(visual, format!("message-text-{turn}"));
            drag(
                visual,
                point(text_bounds.left() + px(1.), text_bounds.center().y),
                point(text_bounds.left() + px(70.), text_bounds.center().y),
            );
            let selected = visual.update(gpui_kit::base::TextSelection::selected_text);
            assert!(!selected.is_empty(), "Kit selection expected for {action}");
            let excerpt = view.read_with(visual, |view, _| {
                view.selection
                    .as_ref()
                    .expect("selection actions expected")
                    .text
                    .clone()
            });
            assert!(
                !excerpt.is_empty() && text.starts_with(&excerpt) && excerpt != text,
                "{action}: captured {excerpt:?}, selected {selected:?}"
            );
            assert!(visual.debug_bounds("message-selection-actions").is_some());
            if action == "quote" {
                visual.update(|window, cx| {
                    view.update(cx, |view, cx| {
                        view.input
                            .update(cx, |input, cx| input.set_value("Unsent draft", window, cx))
                    })
                });
            }
            tap(visual, &format!("message-selection-{action}"));
            wait(visual, |_| true);
            assert!(visual.debug_bounds("message-selection-actions").is_none());
            match action {
                "copy" => assert_eq!(
                    visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
                    excerpt
                ),
                "quote" => assert_eq!(
                    view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
                    format!("Unsent draft\n\n> {excerpt}\n\n")
                ),
                _ => new = Some(excerpt),
            }
        }
        assert_eq!(*events.borrow(), [new.unwrap()]);
        let answer = bounds(visual, format!("live-turn-text-{turn}"));
        drag(
            visual,
            point(answer.left() + px(1.), answer.top() + px(10.)),
            point(answer.left() + px(35.), answer.top() + px(10.)),
        );
        let excerpt = view.read_with(visual, |view, _| {
            view.selection.as_ref().unwrap().text.clone()
        });
        assert!(!excerpt.is_empty() && excerpt != text);
        tap(visual, "message-selection-copy");
        wait(visual, |_| true);
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            excerpt
        );
        assert!(visual.debug_bounds("message-selection-actions").is_none());
        assert_eq!(fixture.task_requests(), 1);
        fixture.close();
    }
}

#[gpui::test]
fn quotes_preserve_editing_and_recovery(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        let draft = "> Manually typed **Markdown** 中文 🙂";
        let excerpt = "Selected **Markdown** 中文 🙂\n\n\u{301}Leading mark";
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.input
                    .update(cx, |input, cx| input.set_value(draft, window, cx));
                assert!(view.insert_quote(excerpt, window, cx));
            });
        });
        let original = view.read_with(visual, |view, cx| view.input.read(cx).content());
        let expected =
            format!("{draft}\n\n> Selected **Markdown** 中文 🙂\n> \n> \u{301}Leading mark\n\n");
        assert_eq!(original.text().as_str(), expected);
        assert_eq!(original.tokens().len(), 3);
        assert!(original.tokens().iter().all(|span| {
            span.token().text() == ">"
                && span.token().label() == " "
                && span.range().start > draft.len()
        }));
        assert!(view.read_with(visual, |view, cx| view.active_references(cx).is_empty()));
        visual.simulate_keystrokes("cmd-z");
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value().as_str(), draft);
            assert!(view.input.read(cx).tokens().is_empty());
        });
        visual.simulate_keystrokes("cmd-shift-z");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).content()),
            original
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let start = expected.find("Selected").unwrap();
                view.input.update(cx, |input, cx| {
                    input.set_selected_range(start..start + "Selected".len(), cx);
                    input.replace("Edited", window, cx);
                });
            });
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).tokens().len()),
            3
        );
        visual.simulate_keystrokes("cmd-z");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).content()),
            original
        );
        visual.update(|window, cx| {
            let snapshot = view.read(cx).snapshot_draft(cx).unwrap().unwrap();
            assert_eq!(snapshot.draft.tokens.len(), 3);
            let restored =
                serde_json::from_value(serde_json::to_value(&snapshot.draft).unwrap()).unwrap();
            view.update(cx, |view, cx| {
                view.input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                assert!(view.recover_draft(
                    super::super::recovery::Snapshot {
                        draft: restored,
                        sources: snapshot.sources,
                    },
                    window,
                    cx
                ));
                assert_eq!(view.input.read(cx).content(), original);
            });
        });
        assert_eq!(fixture.task_requests(), 0);
        tap(visual, "live-chat-send");
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
            assert!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .entries
                    .iter()
                    .any(|entry| {
                        entry
                            .parts
                            .contains(&Part::Text(expected.trim().to_owned()))
                    })
            );
        });
        assert_eq!(fixture.task_requests(), 1);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
