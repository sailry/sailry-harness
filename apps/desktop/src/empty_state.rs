//! Existing panel, list and card placements share Kit's native empty-state slots.
use gpui_kit::component::empty::{Empty, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{component::*, *};

pub(crate) fn panel(icon: IconName, label: &'static str, cx: &App) -> Div {
    content(icon.into(), crate::tr(label), label.into(), cx)
}

pub(crate) fn content(icon: Icon, label: SharedString, id: SharedString, cx: &App) -> Div {
    v_flex()
        .size_full()
        .min_h_0()
        .items_center()
        .justify_center()
        .p_6()
        .child(message(icon, label, id, None, cx).max_w_80())
}

pub(crate) fn list(icon: IconName, label: &'static str, cx: &App) -> Div {
    list_content(icon.into(), crate::tr(label), label.into(), cx)
}

pub(crate) fn list_with_text_size(
    icon: Icon,
    label: &'static str,
    text_size: Pixels,
    cx: &App,
) -> Div {
    message(icon, crate::tr(label), label.into(), Some(text_size), cx)
        .flex_shrink_0()
        .py_10()
}

pub(crate) fn list_content(icon: Icon, label: SharedString, id: SharedString, cx: &App) -> Div {
    message(icon, label, id, None, cx).flex_shrink_0().py_10()
}

fn message(
    icon: Icon,
    label: SharedString,
    id: SharedString,
    text_size: Option<Pixels>,
    cx: &App,
) -> Div {
    let title_id = id.clone();
    let icon_id = id.clone();
    v_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .justify_center()
        .gap_3()
        .text_center()
        .debug_selector(move || format!("empty-{id}"))
        .child(
            Empty::new().flex_none().p_0().header(
                EmptyHeader::new()
                    .max_w_full()
                    .gap_3()
                    .media(
                        EmptyMedia::new()
                            .with_variant(EmptyMediaVariant::Icon)
                            .size_12()
                            .rounded_full()
                            .mb_0()
                            .child(
                                h_flex()
                                    .debug_selector(move || format!("empty-icon-{icon_id}"))
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .child(icon.size_6()),
                            ),
                    )
                    .title(
                        EmptyTitle::new()
                            .when_some(text_size, |title, text_size| title.text_size(text_size))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                div()
                                    .debug_selector(move || format!("empty-title-{title_id}"))
                                    .child(label),
                            ),
                    ),
            ),
        )
}

pub(crate) fn card(icon: IconName, label: &'static str, cx: &App) -> Div {
    card_content(list(icon, label, cx), label.into(), cx)
}

pub(crate) fn card_content(body: impl IntoElement, id: SharedString, cx: &App) -> Div {
    card_height(body, id, false, cx)
}

pub(crate) fn card_height(
    body: impl IntoElement,
    id: SharedString,
    fill_height: bool,
    cx: &App,
) -> Div {
    card_aligned(body, id, fill_height, false, cx)
}

pub(crate) fn card_aligned(
    body: impl IntoElement,
    id: SharedString,
    fill_height: bool,
    start: bool,
    cx: &App,
) -> Div {
    let mut style = StyleRefinement::default()
        .p_0()
        .gap_0()
        .border_1()
        .border_dashed()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg);
    if fill_height {
        style = style.h_full().min_h_0();
    }
    div()
        .debug_selector(move || format!("empty-card-{id}"))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .when(fill_height, |card| card.h_full().min_h_0())
        .child(
            GroupBox::new()
                .fill()
                .when(fill_height, |card| card.h_full().min_h_0())
                .content_style(style)
                .child(if fill_height {
                    v_flex()
                        .h_full()
                        .min_h_0()
                        .justify_center()
                        .when(start, |body| body.justify_start())
                        .child(body)
                        .into_any_element()
                } else {
                    body.into_any_element()
                }),
        )
}
