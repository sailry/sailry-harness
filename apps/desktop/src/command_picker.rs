//! Shared command-style search surface for Git targets and conversation matches.
use crate::theme::DialogStyle as _;
use gpui_kit::{
    component::{
        ActiveTheme as _, IconName, Sizable as _,
        dialog::Dialog,
        list::{List, ListDelegate, ListItem, ListState},
    },
    *,
};

pub(crate) const ROW_HEIGHT: f32 = 36.;
pub(crate) const GROUP_HEIGHT: f32 = 28.;
const EMPTY_HEIGHT: f32 = 48.;

pub(crate) fn height(rows: usize, groups: usize, window: &Window) -> Pixels {
    extent(rows, groups, ROW_HEIGHT, window)
}

pub(crate) fn extent(rows: usize, groups: usize, row_height: f32, window: &Window) -> Pixels {
    px((rows as f32 * row_height + groups as f32 * GROUP_HEIGHT + 16.).max(EMPTY_HEIGHT))
        .min(body_height(window))
        + search_height(window)
}

pub(crate) fn list<D: ListDelegate>(
    state: &Entity<ListState<D>>,
    placeholder: impl Into<SharedString>,
    window: &Window,
) -> List<D> {
    List::new(state)
        .large()
        .max_h(body_height(window))
        .p_2()
        .text_sm()
        .line_height(relative(1.25))
        .search_placeholder(placeholder)
}

pub(crate) fn intercept<D: ListDelegate>(
    state: &Entity<ListState<D>>,
    enabled: fn(&D, &App) -> Vec<IndexPath>,
    cx: &mut App,
) -> Subscription {
    let owner = state.downgrade();
    // Pinned Kit's searchable List keeps its input private and consumes Enter
    // without forwarding confirmation; its navigation also includes disabled
    // rows. Retain Command's enabled-row navigation only for this focused input.
    cx.intercept_keystrokes(move |event, window, cx| {
        let reverse = match event.keystroke.key.as_str() {
            "enter" if !event.keystroke.modifiers.shift => None,
            "up" if event.keystroke.modifiers == Modifiers::default() => Some(true),
            "down" if event.keystroke.modifiers == Modifiers::default() => Some(false),
            _ => return,
        };
        let Some(state) = owner.upgrade() else {
            return;
        };
        if !state.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        state.update(cx, |state, cx| {
            if state.delegate().loading(cx) {
                return;
            }
            if let Some(reverse) = reverse {
                let choices = enabled(state.delegate(), cx);
                let selected = next(&choices, state.selected_index(), reverse);
                state.set_selected_index(selected, window, cx);
                state.scroll_to_selected_item(window, cx);
                cx.notify();
            } else {
                state
                    .delegate_mut()
                    .confirm(event.keystroke.modifiers.secondary(), window, cx);
            }
        });
        cx.stop_propagation();
    })
}

fn next(choices: &[IndexPath], selected: Option<IndexPath>, reverse: bool) -> Option<IndexPath> {
    if choices.is_empty() {
        return None;
    }
    let index = selected.and_then(|selected| choices.iter().position(|index| *index == selected));
    Some(
        choices[match (index, reverse) {
            (Some(index), true) => (index + choices.len() - 1) % choices.len(),
            (Some(index), false) => (index + 1) % choices.len(),
            (None, true) => choices.len() - 1,
            (None, false) => 0,
        }],
    )
}

pub(crate) fn row(id: impl Into<ElementId>, checked: bool, disabled: bool, cx: &App) -> ListItem {
    ListItem::new(id)
        .h(px(ROW_HEIGHT))
        .px_2()
        .rounded(cx.theme().radius)
        .disabled(disabled)
        .check_icon(IconName::Check)
        .confirmed(checked)
}

pub(crate) fn heading(label: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_2()
        .h_7()
        .flex()
        .items_center()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(label.into())
}

pub(crate) fn dialog<D: ListDelegate>(
    dialog: Dialog,
    id: &'static str,
    list: &Entity<ListState<D>>,
    placeholder: SharedString,
    window: &Window,
    cx: &App,
) -> Dialog {
    frame(
        dialog,
        id,
        self::list(list, placeholder, window),
        window,
        cx,
    )
}

pub(crate) fn body_height(window: &Window) -> Pixels {
    px(344.).min((window.viewport_size().height * 0.6 - search_height(window)).max(px(32.)))
}

fn search_height(window: &Window) -> Pixels {
    window.rem_size() * 2.75 + px(1.)
}

pub(crate) fn width(window: &Window) -> Pixels {
    px(480.).min(window.viewport_size().width - px(40.))
}

pub(crate) fn frame(
    dialog: Dialog,
    id: &'static str,
    content: impl IntoElement,
    window: &Window,
    cx: &App,
) -> Dialog {
    dialog
        .floating_surface(cx)
        .close_button(false)
        .min_h_0()
        .p_0()
        .overflow_hidden()
        .w(width(window))
        .child(
            div()
                .debug_selector(move || id.into())
                .max_h(window.viewport_size().height * 0.6)
                .overflow_hidden()
                .child(content),
        )
}

#[cfg(test)]
mod navigation {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn wraps_enabled_rows() {
        let first = IndexPath::new(1);
        let last = IndexPath::new(3).section(1);
        let choices = [first, last];
        assert_eq!(next(&choices, Some(first), false), Some(last));
        assert_eq!(next(&choices, Some(last), false), Some(first));
        assert_eq!(next(&choices, Some(first), true), Some(last));
        assert_eq!(next(&choices, Some(last), true), Some(first));
    }

    #[test]
    fn starts_at_enabled_edges() {
        let choices = [IndexPath::new(1), IndexPath::new(3)];
        for selected in [None, Some(IndexPath::new(0))] {
            assert_eq!(next(&choices, selected, false), Some(choices[0]));
            assert_eq!(next(&choices, selected, true), Some(choices[1]));
        }
        assert_eq!(next(&[], None, false), None);
        assert_eq!(next(&[], Some(choices[0]), true), None);
    }
}
