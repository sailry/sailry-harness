//! Shared right-pane composition; each catalog kind owns its data and actions.
use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement;

pub(in crate::settings) mod rows;

pub(in crate::settings) mod skills;
pub(in crate::settings::market) mod themes;

pub(in crate::settings) fn skeleton(cx: &App) -> Div {
    v_flex()
        .w_full()
        .min_w_0()
        .gap_5()
        .child(Skeleton::new().w_2_3().h_6().rounded(cx.theme().radius))
        .child(v_flex().gap_3().children((0..4).map(|index| {
            Skeleton::new()
                .secondary()
                .when(index == 3, |line| line.w_3_4())
                .rounded(cx.theme().radius)
        })))
        .child(
            Skeleton::new()
                .secondary()
                .h_24()
                .rounded(cx.theme().radius_lg),
        )
}

pub(in crate::settings) fn panel(
    title: &'static str,
    summary: AnyElement,
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
                .child(div().flex_1().min_w_0().text_sm().child(tr(title)))
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
            div()
                .id("extension-details-body")
                .relative()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .child(
                    v_flex()
                        .w_full()
                        .min_w_0()
                        .pt_5()
                        .pb_7()
                        .gap_5()
                        .child(summary)
                        .child(div().w_full().min_w_0().px_7().child(content)),
                )
                .vertical_scrollbar(scroll),
        )
        .into_any_element()
}

fn summary(
    title: SharedString,
    subtitle: String,
    description: Option<String>,
    icon: Option<AnyElement>,
    action: Option<AnyElement>,
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
                .when_some(action, |row, action| {
                    row.child(div().flex_shrink_0().child(action))
                }),
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
