//! Shared queue composition for preview and connected conversations.
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Enter, Textarea},
    list::ListItem,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub(in crate::conversation) fn trigger(
    id: impl Into<ElementId>,
    count: usize,
    preview: SharedString,
    cx: &App,
) -> Button {
    let label = rust_i18n::t!("queue_count", count = count).to_string();
    Button::new(id)
        .custom(crate::theme::subtle_button(cx))
        .rounded_full()
        .w_full()
        .accessibility_label(label.clone())
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(Icon::new(IconName::Menu).size_4())
                .child(div().flex_shrink_0().child(label))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(preview),
                ),
        )
}

pub(in crate::conversation) fn row(
    id: impl Into<ElementId>,
    key: SharedString,
    label: SharedString,
    attached: bool,
    actions: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> ListItem {
    ListItem::new(id)
        .h_10()
        .px_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .id(SharedString::from(format!("queue-drag-{key}")))
                        .size_6()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_grab()
                        .debug_selector(move || format!("queue-drag-{key}"))
                        .aria_label(tr("queue_drag"))
                        .child(Icon::new(IconName::EllipsisVertical).size_4()),
                )
                .child(div().flex_1().min_w_0().truncate().text_sm().child(label))
                .when(attached, |row| {
                    row.child(Icon::new(IconName::File).size_4())
                })
                .children(actions),
        )
}

pub(in crate::conversation) fn editor(
    input: Textarea,
    actions: impl IntoIterator<Item = AnyElement>,
) -> Div {
    v_flex()
        .gap_2()
        .p_1()
        .child(
            div()
                .id("queue-edit-field")
                .debug_selector(|| "queue-input".into())
                .on_action(|action: &Enter, _, cx| {
                    if action.shift {
                        cx.propagate();
                    }
                })
                .child(input.aria_label(tr("queue_edit"))),
        )
        .child(h_flex().justify_end().gap_2().children(actions))
}

pub(in crate::conversation) fn panel(window: &Window) -> Div {
    v_flex()
        .debug_selector(|| "queue-panel".into())
        .w(px(480.).min(window.viewport_size().width - px(48.)))
        .max_h(px(280.).min(window.viewport_size().height * 0.6))
        .gap_2()
}

pub(in crate::conversation) fn drag(text: SharedString, cx: &App) -> ListItem {
    ListItem::new("queue-drag-preview")
        .selected(true)
        .w_64()
        .h_10()
        .rounded(cx.theme().radius)
        .child(div().truncate().child(text))
}
