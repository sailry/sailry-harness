//! Shared right-pane composition; each catalog kind owns its data and actions.
use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement;

pub(in crate::settings) mod rows;

pub(super) mod skills;
pub(super) mod themes;

pub(in crate::settings) fn panel(
    summary: AnyElement,
    information: AnyElement,
    content: AnyElement,
    scroll: &ScrollHandle,
    cx: &App,
) -> AnyElement {
    v_flex()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .text_sm()
        .font_normal()
        .child(
            crate::header::Header::new("extension-details-header", cx)
                .justify_end()
                .child(
                    Button::new("extension-details-close")
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .accessibility_label(tr("close"))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(super::super::CloseDetails), cx)
                        }),
                ),
        )
        .child(
            v_flex()
                .flex_shrink_0()
                .min_w_0()
                .pt_5()
                .gap_4()
                .child(summary)
                .child(div().px_7().child(information)),
        )
        .child(
            div()
                .id("extension-details-body")
                .relative()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .child(div().w_full().min_w_0().px_7().pt_5().pb_7().child(content))
                .vertical_scrollbar(scroll),
        )
        .into_any_element()
}

fn summary(
    title: SharedString,
    subtitle: String,
    description: Option<String>,
    icon: Option<AnyElement>,
    action: Option<Button>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .w_full()
        .min_w_0()
        .px_7()
        .gap_3()
        .child(
            h_flex()
                .min_w_0()
                .gap_3()
                .children(icon)
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(div().text_lg().font_semibold().truncate().child(title))
                        .when(!subtitle.is_empty(), |view| {
                            view.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(subtitle),
                            )
                        }),
                )
                .children(action.map(|action| action.flex_shrink_0())),
        )
        .when_some(description, |view, description| {
            view.child(
                div()
                    .text_sm()
                    .line_height(relative(1.6))
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
        })
        .into_any_element()
}
