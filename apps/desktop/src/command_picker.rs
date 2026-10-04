//! Shared command-style search surface for Git targets and conversation matches.
use gpui_kit::{
    component::{
        ActiveTheme as _, IconName,
        dialog::Dialog,
        list::{List, ListDelegate, ListItem, ListState},
    },
    *,
};

pub(crate) const ROW_HEIGHT: f32 = 36.;
pub(crate) const GROUP_HEIGHT: f32 = 28.;
const SEARCH_HEIGHT: f32 = 45.;
const EMPTY_HEIGHT: f32 = 48.;

pub(crate) fn height(rows: usize, groups: usize, window: &Window) -> Pixels {
    px((rows as f32 * ROW_HEIGHT + groups as f32 * GROUP_HEIGHT + 16.).max(EMPTY_HEIGHT))
        .min(body_height(window))
        + px(SEARCH_HEIGHT)
}

pub(crate) fn list<D: ListDelegate>(
    state: &Entity<ListState<D>>,
    placeholder: impl Into<SharedString>,
    window: &Window,
) -> List<D> {
    List::new(state)
        .max_h(body_height(window))
        .p_2()
        .text_sm()
        .line_height(relative(1.25))
        .search_placeholder(placeholder)
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
) -> Dialog {
    frame(dialog, id, self::list(list, placeholder, window), window)
}

pub(crate) fn body_height(window: &Window) -> Pixels {
    px(344.).min((window.viewport_size().height * 0.6 - px(45.)).max(px(32.)))
}

pub(crate) fn width(window: &Window) -> Pixels {
    px(480.).min(window.viewport_size().width - px(40.))
}

pub(crate) fn frame(
    dialog: Dialog,
    id: &'static str,
    content: impl IntoElement,
    window: &Window,
) -> Dialog {
    dialog
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
