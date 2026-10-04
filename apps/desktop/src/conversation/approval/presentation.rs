//! Shared composition for the approved pending-request area.
use gpui_kit::component::{scroll::ScrollableElement, separator::Separator, *};
use gpui_kit::*;

pub(crate) fn panel(
    id: impl Into<SharedString>,
    heading: SharedString,
    children: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .id("pending-approvals")
        .debug_selector(|| "pending-approvals".into())
        .w_full()
        .h_48()
        .max_h_48()
        .min_h_0()
        .child(
            v_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .gap_2()
                .overflow_y_scrollbar()
                .id(id.into())
                .child(Separator::horizontal())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(heading),
                )
                .children(children),
        )
        .into_any_element()
}

pub(crate) fn row(prompt: SharedString, actions: impl IntoElement) -> Div {
    h_flex()
        .w_full()
        .gap_2()
        .items_start()
        .flex_wrap()
        .child(Icon::new(IconName::CircleUser).size_4())
        .child(
            div()
                .min_w_0()
                .flex_1()
                .min_h_0()
                .text_sm()
                .whitespace_normal()
                .child(prompt),
        )
        .child(actions)
}
