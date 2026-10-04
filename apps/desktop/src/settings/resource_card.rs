//! Shared installed-resource layout, composed from Kit controls.
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonCustomVariant, ButtonVariants},
        group_box::{GroupBox, GroupBoxVariants},
        *,
    },
    *,
};

pub(super) fn grid() -> Div {
    div().grid().grid_cols(2).gap_3().w_full()
}

pub(super) fn summary(
    id: String,
    title: String,
    description: String,
    icon: AnyElement,
    cx: &App,
) -> Button {
    let text_id = id
        .replace("-details-", "-summary-")
        .replace("-edit-", "-summary-");
    Button::new(SharedString::from(id.clone()))
        .custom(ButtonCustomVariant::new(cx))
        .debug_selector(move || id.clone())
        .accessibility_label(title.clone())
        .flex_1()
        .min_w_0()
        .h_auto()
        .p_0()
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_3()
                .child(div().flex_shrink_0().child(icon))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .items_start()
                        .gap_1()
                        .debug_selector(move || text_id.clone())
                        .child(div().w_full().truncate().font_semibold().child(title))
                        .child(
                            div()
                                .w_full()
                                .line_clamp(2)
                                .overflow_hidden()
                                .text_sm()
                                .whitespace_normal()
                                .text_color(cx.theme().muted_foreground)
                                .opacity(0.75)
                                .child(description),
                        ),
                ),
        )
}

pub(super) fn menu(id: String, group: SharedString) -> Button {
    Button::new(SharedString::from(id.clone()))
        .ghost()
        .small()
        .icon(IconName::Ellipsis)
        .accessibility_label(tr("plugins_actions"))
        .debug_selector(move || id.clone())
        .opacity(0.)
        .group_hover(group, |style| style.opacity(1.))
        .focus_visible(|style| style.opacity(1.))
}

pub(super) fn card(id: String, summary: Button, actions: impl IntoElement, cx: &App) -> AnyElement {
    GroupBox::new()
        .fill()
        .min_w_0()
        .content_style(
            StyleRefinement::default()
                .h_full()
                .justify_center()
                .px_4()
                .py_3()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius_lg),
        )
        .child(
            h_flex()
                .group(SharedString::from(id.clone()))
                .debug_selector(move || id.clone())
                .min_w_0()
                .items_center()
                .gap_3()
                .child(summary)
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_2()
                        .items_center()
                        .child(actions),
                ),
        )
        .into_any_element()
}
