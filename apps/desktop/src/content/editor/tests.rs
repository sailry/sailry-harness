use super::*;
use crate::content::markdown::Part;
use gpui_kit as gpui;
use gpui_kit::component::Root;
use gpui_kit::{EntityInputHandler, Modifiers, TestAppContext, VisualTestContext};

fn setup<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
) -> (Entity<State>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut state = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let editor = cx.new(|cx| State::new(source.to_owned(), window, cx));
        state = Some(editor.clone());
        Root::new(editor, window, cx)
    });
    settle(visual);
    (state.unwrap(), visual)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn switches_modes(cx: &mut TestAppContext) {
    let source = "> - **original**\r\n\r\n[ref]: https://example.com \"Title\"\r\n";
    let (state, visual) = setup(cx, source);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.set_mode(Mode::Source, window, cx);
            state.set_mode(Mode::Document, window, cx);
            assert_eq!(state.source(cx).as_ref(), source);
            state.selection =
                Selection::new(Cursor::new(0, Part::Body, 0), Cursor::new(0, Part::Body, 8));
            state.insert("edited 中文", window, cx);
            assert_eq!(state.mode(), Mode::Document);
            assert_eq!(
                state.source(cx).as_ref(),
                "> - **edited 中文**\r\n\r\n[ref]: https://example.com \"Title\"\r\n"
            );
            state.set_mode(Mode::Source, window, cx);
            state.undo(&Undo, window, cx);
            assert_eq!(state.source(cx).as_ref(), source);
            state.redo(&Redo, window, cx);
            assert!(state.source(cx).contains("edited 中文"));
        })
    });
}

#[gpui::test]
fn rendered_caret_projects_into_the_source_buffer(cx: &mut TestAppContext) {
    let (state, visual) = setup(cx, "# Title\n\n**é🚢x**\n");
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.selection = Selection::at(Cursor::new(1, Part::Body, "é🚢".len()));
            assert_eq!(state.source_cursor_position(cx), Some(Position::new(2, 4)));
            let Buffer::Editor(input) = &state.input else {
                panic!("expected the existing document editor");
            };
            assert_eq!(input.read(cx).cursor_position(), Position::new(0, 0));
            state.focus_handle(cx).focus(window, cx);
        });
    });
    visual.simulate_keystrokes("left");
    settle(visual);
    state.read_with(visual, |state, cx| {
        assert_eq!(state.source_cursor_position(cx), Some(Position::new(2, 3)));
    });
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.set_mode(Mode::Source, window, cx);
            assert_eq!(state.source_cursor_position(cx), None);
            let Buffer::Editor(input) = &state.input else {
                panic!("expected the existing document editor");
            };
            assert_eq!(input.read(cx).cursor_position(), Position::new(2, 3));
        });
    });
}

#[gpui::test]
fn pointer_editing(cx: &mut TestAppContext) {
    let source = "Plain **bold** and *italic* tail\r\n\r\n[ref]: notes.md\r\n";
    let (state, visual) = setup(cx, source);
    let point = visual.update(|_, cx| {
        let state = state.read(cx);
        let at = Cursor::new(0, Part::Body, "Plain bold and italic tail".len());
        let (point, height) = state.layouts.position(at).unwrap();
        point + gpui_kit::point(px(2.), height / 2.)
    });
    visual.simulate_mouse_down(point, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
    visual.simulate_input(" changed 中文");
    settle(visual);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            assert_eq!(state.mode(), Mode::Document);
            assert_eq!(
                state.source(cx).as_ref(),
                source.replacen("tail", "tail changed 中文", 1)
            );
            state.press(
                &gpui_kit::MouseDownEvent {
                    button: MouseButton::Left,
                    position: point,
                    click_count: 3,
                    ..Default::default()
                },
                window,
                cx,
            );
            state.command(Command::Copy, window, cx);
            assert_eq!(
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .as_deref(),
                Some("Plain bold and italic tail changed 中文")
            );
            state.command(Command::Undo, window, cx);
            assert_eq!(state.source(cx).as_ref(), source);
            state.command(Command::Redo, window, cx);
            assert!(state.source(cx).contains("changed 中文"));
        })
    });
}

#[gpui::test]
fn edits_structured_blocks(cx: &mut TestAppContext) {
    let source = "| Name | Value |\r\n| --- | --- |\r\n| one | **two** |\r\n\r\n~~~rust\r\nlet value = 1;\r\n~~~\r\n\r\n[ref]: notes.md\r\n";
    let (state, visual) = setup(cx, source);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.place(Cursor::new(0, Part::Cell { row: 1, column: 1 }, 1));
            state.replace_text_in_range(None, "中文", window, cx);
            assert_eq!(
                state.source(cx).as_ref(),
                source.replacen("**two**", "**t中文wo**", 1)
            );
            let cell = state.source(cx);
            state.enter(&keys::Enter, window, cx);
            assert_eq!(state.source(cx), cell);
            state.commit(cx);
            state.place(Cursor::new(1, Part::Code, 4));
            state.replace_text_in_range(None, "new_", window, cx);
            assert_eq!(
                state.source(cx).as_ref(),
                cell.replacen("let value", "let new_value", 1)
            );
            assert_eq!(state.mode(), Mode::Document);
            state.undo(&Undo, window, cx);
            assert_eq!(state.source(cx), cell);
            state.undo(&Undo, window, cx);
            assert_eq!(state.source(cx).as_ref(), source);
        })
    });
}

#[gpui::test]
fn groups_typing_undo(cx: &mut TestAppContext) {
    let source = "# Original\r\n\r\n[ref]: notes.md\r\n";
    let (state, visual) = setup(cx, source);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.place(Cursor::new(0, Part::Body, 8));
            for part in [" ", "Revised", " ", "中", "文"] {
                state.replace_text_in_range(None, part, window, cx);
            }
            assert_eq!(
                state.source(cx).as_ref(),
                source.replacen("Original", "Original Revised 中文", 1)
            );
            assert_eq!(state.caret_text().unwrap().text, "Original Revised 中文");
            state.undo(&Undo, window, cx);
            assert_eq!(state.source(cx).as_ref(), source);
            state.redo(&Redo, window, cx);
            assert!(state.source(cx).contains("Original Revised 中文"));
        })
    });
}

#[gpui::test]
fn structural_keys(cx: &mut TestAppContext) {
    let source = "> 3. beforeafter\r\n\r\nOutside\r\n\r\n[ref]: notes.md\r\n";
    let (state, visual) = setup(cx, source);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.place(Cursor::new(0, Part::Body, 6));
            state.enter(&keys::Enter, window, cx);
            assert_eq!(state.doc.blocks.len(), 3);
            assert_eq!(parse(&state.source(cx)), state.doc);
            assert!(
                state
                    .source(cx)
                    .ends_with("\r\n\r\nOutside\r\n\r\n[ref]: notes.md\r\n")
            );
            state.undo(&Undo, window, cx);
            assert_eq!(state.source(cx).as_ref(), source);
        })
    });
}

#[gpui::test]
fn cancels_stale_composition(cx: &mut TestAppContext) {
    let (state, visual) = setup(cx, "# Original\n");
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.place(Cursor::new(0, Part::Body, 8));
            state.replace_and_mark_text_in_range(None, "中", None, window, cx);
            assert_eq!(state.source(cx).as_ref(), "# Original\n");
            let Buffer::Editor(input) = &state.input else {
                unreachable!()
            };
            input.update(cx, |input, cx| {
                input.set_value("# External\n", window, cx);
            });
            state.sync(cx);
            assert!(state.marked.is_none());
            assert_eq!(state.source(cx).as_ref(), "# External\n");
            state.readonly = true;
            state.replace_text_in_range(None, "bad", window, cx);
            state.backspace(&keys::Backspace, window, cx);
            assert_eq!(state.source(cx).as_ref(), "# External\n");
        })
    });
}

#[gpui::test]
fn clipboard_focus(cx: &mut TestAppContext) {
    let (state, visual) = setup(cx, "original");
    visual.update(|window, cx| {
        state.update(cx, |state, _| state.selection = Selection::all(&state.doc));
        window.blur(cx);
        state.update(cx, |state, cx| {
            state.command(Command::Copy, window, cx);
            assert!(state.focus.is_focused(window));
            assert_eq!(
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .as_deref(),
                Some("original")
            );
        });
        window.blur(cx);
        state.update(cx, |state, cx| {
            state.command(Command::Cut, window, cx);
            assert!(state.focus.is_focused(window));
            assert_eq!(state.source(cx).as_ref(), "");
        });
        window.blur(cx);
        state.update(cx, |state, cx| {
            state.command(Command::Paste, window, cx);
            assert!(state.focus.is_focused(window));
            assert_eq!(state.source(cx).as_ref(), "original");
        });
    });
    settle(visual);
    visual.simulate_input("x");
    visual.update(|_, cx| assert_eq!(state.read(cx).source(cx).as_ref(), "originalx"));
}

#[gpui::test]
fn shares_menu_selection(cx: &mut TestAppContext) {
    let (state, visual) = setup(cx, "# Original\n\nBody\n");
    visual.update(|window, cx| state.update(cx, |state, cx| state.focus.focus(window, cx)));
    settle(visual);
    visual.dispatch_action(SelectAll);
    visual.dispatch_action(Copy);
    settle(visual);
    assert_eq!(
        visual.read_from_clipboard().unwrap().text().as_deref(),
        Some("Original\nBody")
    );
    assert!(state.read_with(visual, |state, _| !state.selection.is_collapsed()));
}

#[gpui::test]
fn preserves_literal_prefixes(cx: &mut TestAppContext) {
    let original = "\\# Heading\r\n\r\n\\- item\r\n\r\n[ref]: notes.md\r\n";
    let (state, visual) = setup(cx, original);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.place(Cursor::new(0, Part::Body, "# Heading".len()));
            state.command(Command::Cut, window, cx);
            assert_eq!(state.source(cx).as_ref(), original);
            state.insert("x", window, cx);
            assert_eq!(
                state.source(cx).as_ref(),
                original.replacen("Heading", "Headingx", 1)
            );
            assert!(matches!(state.doc.blocks[0].kind, BlockKind::Paragraph(_)));
            state.commit(cx);
            state.place(Cursor::new(1, Part::Body, "- item".len()));
            state.insert("x", window, cx);
            assert_eq!(
                state.source(cx).as_ref(),
                original
                    .replacen("Heading", "Headingx", 1)
                    .replacen("item", "itemx", 1)
            );
            assert!(matches!(state.doc.blocks[1].kind, BlockKind::Paragraph(_)));
            state.undo(&Undo, window, cx);
            state.undo(&Undo, window, cx);
            assert_eq!(state.source(cx).as_ref(), original);
        })
    });
}

#[gpui::test]
fn types_heading_prefixes(cx: &mut TestAppContext) {
    let (state, visual) = setup(cx, "");
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            for part in ["#", " ", "Title"] {
                state.insert(part, window, cx);
            }
            assert_eq!(state.source(cx).as_ref(), "# Title");
            assert!(matches!(
                state.doc.blocks[0].kind,
                BlockKind::Heading { level: 1, .. }
            ));
        })
    });
}

#[gpui::test]
fn paste_formats(cx: &mut TestAppContext) {
    let (state, visual) = setup(cx, "beforeafter");
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.place(Cursor::new(0, Part::Body, 6));
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(" a ".into()));
            state.command(Command::Paste, window, cx);
            assert_eq!(state.source(cx).as_ref(), "before a after");
            state.undo(&Undo, window, cx);
            state.selection =
                Selection::new(Cursor::new(0, Part::Body, 0), Cursor::new(0, Part::Body, 6));
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("   ".into()));
            state.command(Command::Paste, window, cx);
            assert_eq!(state.source(cx).as_ref(), "   after");
            state.undo(&Undo, window, cx);
            state.selection = Selection::all(&state.doc);
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("".into()));
            state.command(Command::Paste, window, cx);
            assert_eq!(state.source(cx).as_ref(), "beforeafter");
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                "## Title\n\n- **bold**".into(),
            ));
            state.command(Command::Paste, window, cx);
            assert!(matches!(
                state.doc.blocks[0].kind,
                BlockKind::Heading { level: 2, .. }
            ));
            assert!(matches!(state.doc.blocks[1].kind, BlockKind::Bullet(_)));
            assert_eq!(parse(&state.source(cx)), state.doc);
            state.undo(&Undo, window, cx);
            assert_eq!(state.source(cx).as_ref(), "beforeafter");
        })
    });
}
