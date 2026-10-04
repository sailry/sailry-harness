use super::*;
use crate::content::editor::Command;
use gpui_kit as gpui;
use gpui_kit::{TestAppContext, VisualTestContext};

fn open<'a>(
    source: &'static str,
    cx: &'a mut TestAppContext,
) -> (gpui::Entity<State>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    cx.add_window_view(|window, cx| State::new(source, window, cx))
}

#[gpui::test]
fn utf16_composition(cx: &mut TestAppContext) {
    let (state, visual) = open("中😀abc\r\n\r\n[ref]: https://example.com\r\n", cx);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            let original = state.source(cx);
            let mut adjusted = None;
            assert_eq!(
                state
                    .text_for_range(1..3, &mut adjusted, window, cx)
                    .as_deref(),
                Some("😀")
            );
            assert_eq!(adjusted, Some(1..3));
            state.replace_and_mark_text_in_range(Some(1..3), "文", Some(0..1), window, cx);
            assert_eq!(state.source(cx), original);
            assert_eq!(state.marked_text_range(window, cx), Some(1..2));
            state.replace_and_mark_text_in_range(None, "文字", Some(1..2), window, cx);
            assert_eq!(state.source(cx), original);
            assert_eq!(state.marked_text_range(window, cx), Some(1..3));
            assert_eq!(
                state.selected_text_range(false, window, cx).unwrap().range,
                2..3
            );
            state.replace_text_in_range(None, "文", window, cx);
            assert_eq!(
                state.source(cx).as_ref(),
                "中文abc\r\n\r\n[ref]: https://example.com\r\n"
            );
            assert_eq!(state.marked_text_range(window, cx), None);
            state.command(Command::Undo, window, cx);
            assert_eq!(state.source(cx), original);
        });
    });
}

#[gpui::test]
fn composition_completion(cx: &mut TestAppContext) {
    let (state, visual) = open("original", cx);
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.replace_and_mark_text_in_range(Some(0..8), "中文", None, window, cx);
            assert_eq!(state.source(cx).as_ref(), "original");
            state.unmark_text(window, cx);
            assert_eq!(state.source(cx).as_ref(), "中文");
            state.unmark_text(window, cx);
            state.command(Command::Undo, window, cx);
            assert_eq!(state.source(cx).as_ref(), "original");

            state.replace_and_mark_text_in_range(Some(0..8), "candidate", None, window, cx);
            state.replace_and_mark_text_in_range(None, "", None, window, cx);
            assert_eq!(state.source(cx).as_ref(), "original");
            assert_eq!(state.caret_text().unwrap().text, "original");
            assert_eq!(state.marked_text_range(window, cx), None);
            assert!(state.composition.is_none());
        });
    });
}
