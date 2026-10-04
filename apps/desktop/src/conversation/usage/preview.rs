//! Explicit preview values do not subscribe to a Node or execute package code.
use super::*;
use gpui_kit::component::progress::ProgressCircle;

pub(super) fn group(turns: usize) -> Group {
    Group {
        metrics: [
            ("composer_tokens", Some(IconName::ChartPie), "—".to_owned()),
            ("composer_speed", Some(IconName::ArrowUp), "—".to_owned()),
            ("composer_cost", None, "$—".to_owned()),
            (
                "composer_cache",
                Some(IconName::MemoryStick),
                "—".to_owned(),
            ),
            (
                "composer_turns",
                Some(IconName::RotateCw),
                turns.to_string(),
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (id, icon, value))| Metric {
            id: id.into(),
            order: index as i16 * 10,
            value: value.into(),
            icon: icon.map(Into::into),
            prefix: None,
            details: Rc::new(|_| div().child(tr("preview")).into_any_element()),
        })
        .collect(),
        summary: Rc::new(|_| div().child(tr("preview")).into_any_element()),
    }
}

pub(in crate::conversation) fn context_progress(cx: &App) -> impl IntoElement {
    div()
        .id("composer-compression")
        .debug_selector(|| "composer-compression".into())
        .flex_shrink_0()
        .px_1()
        .tooltip(|window, cx| Tooltip::new(tr("composer_compression_hint")).build(window, cx))
        .child(
            ProgressCircle::new("composer-context")
                .value(0.)
                .with_size(px(20.))
                .color(cx.theme().muted_foreground)
                .accessibility_label(tr("composer_compression_hint")),
        )
}
