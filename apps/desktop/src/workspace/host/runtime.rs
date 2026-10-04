use crate::{shell::Shell, tr};
use gpui_kit::component::{
    description_list::{DescriptionItem, DescriptionList},
    shimmer::ShimmerText,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

impl Shell {
    pub(super) fn host_runtime(&self, cx: &App) -> AnyElement {
        let snapshot = self
            .workspace
            .runtimes
            .get(&self.host)
            .cloned()
            .unwrap_or_default();
        v_flex()
            .gap_2()
            .child(div().font_semibold().child(tr("workspace_runtime")))
            .when(snapshot.loading, |body| {
                body.child(
                    div()
                        .id("host-runtime-loading-label")
                        .debug_selector(|| "host-runtime-loading".into())
                        .aria_label(tr("host_runtime_loading"))
                        .child(
                            ShimmerText::new(tr("host_runtime_loading")).id("host-runtime-loading"),
                        ),
                )
            })
            .child(div().debug_selector(|| "host-runtime-facts".into()).child(
                DescriptionList::new().columns(1).bordered(false).children(
                    snapshot.facts().into_iter().map(|(key, value)| {
                        DescriptionItem::new(tr(key)).value(
                            div()
                                .id(key)
                                .truncate()
                                .aria_label(value.clone())
                                .debug_selector(move || format!("host-runtime-{key}"))
                                .child(value)
                                .into_any_element(),
                        )
                    }),
                ),
            ))
            .child(div().text_color(cx.theme().muted_foreground).child(tr(
                if self.workspace.runtimes.contains_key(&self.host) {
                    "host_runtime_preview"
                } else {
                    "workspace_runtime_preview"
                },
            )))
            .into_any_element()
    }
}
