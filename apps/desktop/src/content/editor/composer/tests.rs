use super::*;
use crate::content::editor::{Command, Mode};
use crate::content::markdown::Part;
use gpui_kit as gpui;
use gpui_kit::component::{
    Root,
    input::{Copy, Redo, SelectAll, TextareaState, Undo},
};
use gpui_kit::prelude::*;
use gpui_kit::{AppContext, ClipboardItem, Entity, Focusable, TestAppContext, VisualTestContext};
use std::{cell::RefCell, rc::Rc};

fn setup<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
) -> (
    Entity<State>,
    Entity<TextareaState>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut state = None;
    let mut buffer = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .submit_on_enter(true)
                .default_value(source.to_owned())
        });
        input.update(cx, |input, cx| {
            input.set_selected_range(source.len()..source.len(), cx)
        });
        let editor = cx.new(|cx| State::from_textarea(&input, cx));
        editor.focus_handle(cx).focus(window, cx);
        state = Some(editor.clone());
        buffer = Some(input);
        Root::new(editor, window, cx)
    });
    settle(visual);
    (state.unwrap(), buffer.unwrap(), visual)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn parses_structure_and_keeps_cursor(cx: &mut TestAppContext) {
    let (state, input, visual) = setup(cx, "plain # text");
    assert!(!state.read_with(visual, |state, _| state.formatted()));
    visual.simulate_keystrokes("cmd-a");
    visual.simulate_input("# Markdown");
    settle(visual);
    state.read_with(visual, |state, cx| {
        assert!(state.formatted());
        assert_eq!(state.mode(), Mode::Document);
        assert_eq!(state.caret_text().unwrap().text, "Markdown");
        assert_eq!(input.read(cx).selected_range(), 10..10);
        assert_eq!(state.cursor(), Cursor::new(0, Part::Body, 8));
    });
    visual.simulate_input(" **bold**");
    settle(visual);
    state.read_with(visual, |state, cx| {
        assert_eq!(state.source(cx).as_ref(), "# Markdown **bold**");
        assert_eq!(state.caret_text().unwrap().text, "Markdown bold");
        assert_eq!(input.read(cx).selected_range(), 19..19);
    });
    visual.dispatch_action(SelectAll);
    visual.simulate_keystrokes("backspace");
    settle(visual);
    assert!(input.read_with(visual, |input, _| input.value().is_empty()));
    assert!(!state.read_with(visual, |state, _| state.formatted()));
    visual.dispatch_action(Undo);
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# Markdown **bold**"
    );
}

#[gpui::test]
fn restored_caret_skips_hidden_prefix(cx: &mut TestAppContext) {
    let (state, input, visual) = setup(cx, "plain");
    visual.update(|window, cx| {
        input.update(cx, |input, cx| input.set_value("# Heading", window, cx))
    });
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.selected_range()),
        2..2
    );
    visual.simulate_input("New ");
    settle(visual);
    state.read_with(visual, |state, cx| {
        assert!(state.formatted());
        assert_eq!(state.source(cx).as_ref(), "# New Heading");
    });
}

#[gpui::test]
fn empty_fences_keep_source_and_history(cx: &mut TestAppContext) {
    let source = "> ~~~rust\r\n> ~~~\r\n";
    let (state, input, visual) = setup(cx, source);
    visual.update(|_, cx| {
        state.update(cx, |state, cx| {
            state.place(Cursor::new(0, Part::Code, 0));
            state.notify_caret(cx);
        })
    });
    settle(visual);
    visual.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("pasted".into()));
        state.update(cx, |state, cx| state.command(Command::Paste, window, cx));
    });
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "> ~~~rust\r\n> pasted\r\n> ~~~\r\n"
    );
    visual.dispatch_action(Undo);
    settle(visual);
    assert_eq!(input.read_with(visual, |input, _| input.value()), source);
    visual.update(|_, cx| {
        state.update(cx, |state, cx| {
            state.place(Cursor::new(0, Part::Code, 0));
            state.notify_caret(cx);
        })
    });
    settle(visual);
    visual.simulate_input("x");
    settle(visual);
    state.read_with(visual, |state, cx| {
        assert_eq!(state.source(cx).as_ref(), "> ~~~rust\r\n> x\r\n> ~~~\r\n");
        assert_eq!(state.caret_text().unwrap().text, "x");
        assert_eq!(input.read(cx).selected_range(), 14..14);
    });
    visual.simulate_input("y");
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "> ~~~rust\r\n> xy\r\n> ~~~\r\n"
    );
    visual.dispatch_action(Undo);
    settle(visual);
    assert_eq!(input.read_with(visual, |input, _| input.value()), source);
}

#[gpui::test]
fn selection_formats_do_not_leave_hidden_typing_state(cx: &mut TestAppContext) {
    let (state, input, visual) = setup(cx, "# Heading\n\nOther");
    visual.update(|_, cx| {
        state.update(cx, |state, cx| {
            state.place(Cursor::new(0, Part::Body, 7));
            state.notify_caret(cx);
        })
    });
    settle(visual);
    visual.simulate_keystrokes("cmd-b");
    assert!(state.read_with(visual, |state, _| state.stored.is_empty()));
    visual.update(|_, cx| {
        state.update(cx, |state, cx| {
            state.selection =
                Selection::new(Cursor::new(0, Part::Body, 0), Cursor::new(0, Part::Body, 7));
            state.notify_caret(cx);
        })
    });
    settle(visual);
    visual.simulate_keystrokes("cmd-b");
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# **Heading**\n\nOther"
    );
    visual.simulate_input("x");
    visual.simulate_input("y");
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# **xy**\n\nOther"
    );
    visual.update(|_, cx| {
        state.update(cx, |state, cx| {
            state.selection =
                Selection::new(Cursor::new(0, Part::Body, 0), Cursor::new(0, Part::Body, 2));
            state.notify_caret(cx);
        })
    });
    settle(visual);
    visual.simulate_keystrokes("cmd-b");
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# xy\n\nOther"
    );
    visual.simulate_input("z");
    visual.simulate_input("q");
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# zq\n\nOther"
    );
    assert!(state.read_with(
        visual,
        |state, _| state.caret_text().unwrap().marks.is_empty() && state.stored.is_empty()
    ));
}

#[gpui::test]
fn accessibility_value_shares_history_and_readonly(cx: &mut TestAppContext) {
    let (state, input, visual) = setup(cx, "# Original");
    let value = gpui_kit::accesskit::ActionData::Value("# Changed".into());
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.accessibility_value(Some(&value), window, cx)
        })
    });
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# Changed"
    );
    visual.dispatch_action(Undo);
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# Original"
    );
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.readonly = true;
            state.accessibility_value(Some(&value), window, cx);
        })
    });
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# Original"
    );
}

#[gpui::test]
fn tab_preserves_framework_focus_flow(cx: &mut TestAppContext) {
    use gpui_kit::component::button::Button;
    use gpui_kit::{Context, IntoElement, Render, Window, div};
    struct Harness(Entity<State>);
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .tab_group()
                .child(self.0.clone())
                .child(Button::new("after-input").label("Next"))
        }
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut state = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .default_value("# Heading")
        });
        let editor = cx.new(|cx| State::from_textarea(&input, cx));
        editor.focus_handle(cx).focus(window, cx);
        state = Some(editor.clone());
        let harness = cx.new(|_| Harness(editor));
        Root::new(harness, window, cx)
    });
    let state = state.unwrap();
    settle(visual);
    visual.simulate_keystrokes("tab");
    settle(visual);
    visual.update(|window, cx| assert!(!state.focus_handle(cx).is_focused(window)));
    visual.simulate_keystrokes("shift-tab");
    settle(visual);
    visual.update(|window, cx| assert!(state.focus_handle(cx).is_focused(window)));
    assert_eq!(
        state.read_with(visual, |state, cx| state.source(cx)),
        "# Heading"
    );
}

#[gpui::test]
fn preserves_source_clipboard_and_history(cx: &mut TestAppContext) {
    let source = "> - **original**\r\n\r\n[ref]: notes.md \"Title\"\r\n";
    let (state, input, visual) = setup(cx, source);
    visual.dispatch_action(SelectAll);
    visual.dispatch_action(Copy);
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(source)
    );
    let replacement = "# Revised\r\n\r\n`code` **bold**\r\n";
    visual.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(replacement.to_owned()));
        state.update(cx, |state, cx| state.command(Command::Paste, window, cx));
    });
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        replacement
    );
    visual.dispatch_action(Undo);
    settle(visual);
    assert_eq!(input.read_with(visual, |input, _| input.value()), source);
    visual.dispatch_action(Redo);
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        replacement
    );
    visual.simulate_keystrokes("cmd-a backspace");
    settle(visual);
    assert!(input.read_with(visual, |input, _| input.value().is_empty()));
}

#[gpui::test]
fn enter_uses_existing_subscription(cx: &mut TestAppContext) {
    let (state, input, visual) = setup(cx, "# Heading");
    let events = Rc::new(RefCell::new(Vec::new()));
    let observed = events.clone();
    let _events = visual.update(|_, cx| {
        cx.subscribe(&input, move |_, event: &InputEvent, _| {
            if let InputEvent::PressEnter { shift, .. } = event {
                observed.borrow_mut().push(*shift);
            }
        })
    });
    visual.simulate_keystrokes("enter");
    settle(visual);
    assert_eq!(*events.borrow(), [false]);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# Heading"
    );
    visual.simulate_keystrokes("shift-enter");
    settle(visual);
    assert_eq!(*events.borrow(), [false, true]);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# Heading\n"
    );
    visual.simulate_input("next");
    settle(visual);
    state.read_with(visual, |state, cx| {
        assert_eq!(state.source(cx).as_ref(), "# Heading\nnext");
        assert_eq!(state.cursor(), Cursor::new(1, Part::Body, 4));
        assert_eq!(input.read(cx).selected_range(), 14..14);
    });
}

#[gpui::test]
fn composition_never_edits_or_submits_early(cx: &mut TestAppContext) {
    let (state, input, visual) = setup(cx, "# Original");
    let events = Rc::new(RefCell::new(0));
    let observed = events.clone();
    let _events = visual.update(|_, cx| {
        cx.subscribe(&input, move |_, event: &InputEvent, _| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                *observed.borrow_mut() += 1;
            }
        })
    });
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.replace_and_mark_text_in_range(None, "中", None, window, cx);
            assert!(state.composing());
            assert_eq!(state.source(cx).as_ref(), "# Original");
            assert_eq!(input.read(cx).selected_range(), 10..10);
        })
    });
    settle(visual);
    visual.simulate_keystrokes("enter");
    settle(visual);
    assert_eq!(*events.borrow(), 0);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.unmark_text(window, cx);
            assert!(!state.composing());
            assert_eq!(state.source(cx).as_ref(), "# Original中");
        })
    });
    settle(visual);
    visual.dispatch_action(Undo);
    settle(visual);
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "# Original"
    );
}

#[gpui::test]
fn native_composition_defers_surface_switch(cx: &mut TestAppContext) {
    let (state, input, visual) = setup(cx, "plain");
    visual.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_selected_range(0..5, cx);
            input.replace_and_mark_text_in_range(None, "# 中", None, window, cx);
        });
    });
    settle(visual);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            assert!(state.formatted());
            assert!(!state.document_active(window, cx));
        })
    });
    visual.update(|window, cx| input.update(cx, |input, cx| input.unmark_text(window, cx)));
    settle(visual);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            assert!(state.document_active(window, cx));
            assert_eq!(state.caret_text().unwrap().text, "中");
            assert_eq!(input.read(cx).value(), "# 中");
        })
    });
}

#[test]
fn empty_rows_follow_parsed_fence_boundaries() {
    let open = draft("```rust\nlet value = 1;\n");
    assert!(matches!(
        open.blocks.last().unwrap().kind,
        BlockKind::Code { .. }
    ));
    let closed = draft("```rust\nlet value = 1;\n```\n");
    assert!(matches!(
        closed.blocks.last().unwrap().kind,
        BlockKind::Paragraph(_)
    ));
}
