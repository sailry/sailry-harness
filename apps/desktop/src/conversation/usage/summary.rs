use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;

pub(in crate::conversation) fn render(groups: Vec<Group>, cx: &App) -> AnyElement {
    let value = groups
        .iter()
        .flat_map(|group| &group.metrics)
        .next()
        .map(|metric| metric.value.clone())
        .unwrap_or_else(|| tr("composer_metric_unknown"));
    let groups = Rc::new(groups);
    h_flex()
        .id("composer-stats-tooltip")
        .flex_shrink_0()
        .hoverable_tooltip(move |window, cx| {
            let groups = groups.clone();
            Tooltip::element(move |window, cx| {
                v_flex()
                    .w((window.viewport_size().width - px(56.)).min(px(280.)))
                    .max_h((window.viewport_size().height - px(80.)).min(px(560.)))
                    .debug_selector(|| "composer-statistics-panel".into())
                    .child(
                        v_flex().flex_1().overflow_hidden().child(
                            v_flex()
                                .id("composer-statistics-scroll")
                                .size_full()
                                .children(groups.iter().enumerate().map(|(index, group)| {
                                    v_flex()
                                        .gap_1p5()
                                        .when(index > 0, |body| body.child(Separator::horizontal()))
                                        .child((group.summary)(cx))
                                }))
                                .overflow_y_scrollbar(),
                        ),
                    )
            })
            .p_3()
            .build(window, cx)
        })
        .child(
            Button::new("composer-stats")
                .custom(crate::theme::subtle_button(cx).foreground(cx.theme().muted_foreground))
                .small()
                .rounded_full()
                .flex_shrink_0()
                .icon(IconName::ChartPie)
                .label(value)
                .accessibility_label(tr("composer_statistics"))
                .debug_selector(|| "composer-stats".into()),
        )
        .into_any_element()
}
