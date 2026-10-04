use crate::{shell::Shell, tr};
use gpui_kit::component::{separator::Separator, tooltip::Tooltip, *};
use gpui_kit::*;

mod preview;
pub(super) use preview::context_progress;
mod external;
mod summary;
pub(super) use external::group as external;
use std::rc::Rc;
pub(super) use summary::render as compact;

pub(super) struct Metric {
    pub id: String,
    pub order: i16,
    pub value: SharedString,
    pub icon: Option<Icon>,
    pub prefix: Option<SharedString>,
    pub details: Rc<dyn Fn(&App) -> AnyElement>,
}

pub(super) struct Group {
    pub metrics: Vec<Metric>,
    pub summary: Rc<dyn Fn(&App) -> AnyElement>,
}

impl Shell {
    pub(super) fn composer_usage(&self, cx: &App) -> impl IntoElement {
        render_groups(
            &[preview::group(
                self.conversations[&(self.host, self.session)].turns.len(),
            )],
            cx,
        )
    }
}

pub(super) fn render_groups(groups: &[Group], cx: &App) -> AnyElement {
    let mut metrics: Vec<_> = groups.iter().flat_map(|group| &group.metrics).collect();
    metrics.sort_by_key(|metric| metric.order);
    h_flex()
        .debug_selector(|| "composer-stats".into())
        .ml_auto()
        .gap_3()
        .max_w_full()
        .min_w_0()
        .flex_wrap()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .children(metrics.into_iter().map(|metric| {
            let id = metric.id.clone();
            let details = metric.details.clone();
            h_flex()
                .id(SharedString::from(id.clone()))
                .debug_selector(move || id.clone())
                .gap_1()
                .tooltip(move |window, cx| {
                    let details = details.clone();
                    Tooltip::element(move |_, cx| details(cx)).build(window, cx)
                })
                .children(metric.icon.clone().map(|icon| icon.small()))
                .children(
                    metric
                        .prefix
                        .clone()
                        .map(|prefix| div().w_3p5().text_center().child(prefix)),
                )
                .child(metric.value.clone())
        }))
        .into_any_element()
}
