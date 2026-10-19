use super::*;
use gpui_kit::assets::IconName;

pub(super) fn content(
    title: SharedString,
    description: String,
    icon: Option<AnyElement>,
    source: Option<SharedString>,
    cx: &App,
) -> AnyElement {
    h_flex()
        .w_full()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap_3()
        .children(icon)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .items_start()
                .gap_1()
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .text_left()
                                .truncate()
                                .font_medium()
                                .child(title),
                        )
                        .when_some(source, |row, source| {
                            row.child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(source),
                            )
                        }),
                )
                .child(
                    div()
                        .w_full()
                        .text_left()
                        .truncate()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
        .into_any_element()
}

pub(super) fn row(
    id: String,
    summary: impl IntoElement,
    action: Button,
    cx: &App,
) -> list::ListItem {
    list::ListItem::new(SharedString::from(id))
        .w_full()
        .h(px(76.))
        .text_sm()
        .min_w_0()
        .px_4()
        .gap_3()
        .items_center()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().group_box)
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_3()
                .child(summary)
                .child(action),
        )
}

pub(super) fn action(id: String, installed: bool, update: bool, cx: &App) -> Button {
    let key = if update {
        "market_update"
    } else if installed {
        "market_installed"
    } else {
        "market_install"
    };
    Button::new(SharedString::from(id))
        .ghost()
        .small()
        .size_8()
        .flex_shrink_0()
        .rounded_md()
        .icon(if update {
            IconName::RefreshCw
        } else if installed {
            IconName::Check
        } else {
            IconName::Download
        })
        .text_color(if update {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        })
        .when(!installed || update, |button| {
            button.bg(if update {
                cx.theme().primary.opacity(0.16)
            } else {
                cx.theme().secondary
            })
        })
        .tooltip(tr(key))
        .accessibility_label(tr(key))
        .disabled(installed && !update)
}

pub(super) fn skeleton(cx: &App) -> AnyElement {
    div()
        .grid()
        .grid_cols(2)
        .gap_3()
        .w_full()
        .children((0usize..6).map(|index| {
            h_flex()
                .id(("market-skeleton", index))
                .h(px(76.))
                .px_4()
                .gap_3()
                .items_center()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius_lg)
                .child(Skeleton::new().size_10().rounded_lg())
                .child(
                    v_flex()
                        .flex_1()
                        .gap_2()
                        .child(Skeleton::new().w_1_2().h_4())
                        .child(Skeleton::new().w_full().h_3()),
                )
        }))
        .into_any_element()
}
