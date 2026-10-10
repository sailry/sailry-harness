use super::*;
use crate::settings::{entry::Entry, group::Group};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::Input,
        select::Select,
    },
    prelude::FluentBuilder as _,
};

impl Render for Library {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.loading || !self.available(cx);
        let downloading = locked || self.target.busy(cx);
        let controls = h_flex()
            .w_full()
            .items_center()
            .gap_3()
            .debug_selector(|| "model-library-controls".into())
            .child(
                div()
                    .debug_selector(|| "model-library-source".into())
                    .w_48()
                    .flex_shrink_0()
                    .child(Select::new(&self.hub).w_full().disabled(locked)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .debug_selector(|| "model-library-query".into())
                    .child(
                        Input::new(&self.query)
                            .id("model-library-query")
                            .w_full()
                            .disabled(locked),
                    ),
            )
            .child(
                Button::new("model-library-search")
                    .ghost()
                    .small()
                    .icon(IconName::Search)
                    .accessibility_label(tr("model_library_search"))
                    .tooltip(tr("model_library_search"))
                    .loading(self.loading && self.repository.is_none())
                    .debug_selector(|| "model-library-search".into())
                    .disabled(locked)
                    .on_click(
                        cx.listener(|library, _, window, cx| library.search(false, window, cx)),
                    ),
            );
        let mut results = Group::new("model_library_results")
            .heading(false)
            .empty(IconName::Search, "model_library_empty");
        for (index, item) in self.items.iter().enumerate() {
            let repository = item.source.repository.clone();
            let owner = cx.entity();
            let status = self.target.source_status(&item.source, cx);
            let installed = matches!(status, ModelStatus::Installed);
            let installing = matches!(status, ModelStatus::Downloading(_) | ModelStatus::Checking)
                || (self.installing.as_ref() == Some(&item.source)
                    && (self.loading || self.target.busy(cx)));
            results = results.child(
                Entry::new(
                    format!("model-library-result-{index}"),
                    v_flex()
                        .gap_1()
                        .child(div().font_medium().child(item.name.clone()))
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(repository.clone()),
                        ),
                )
                .action(
                    Button::new(format!("model-library-details-{index}"))
                        .icon(IconName::Info)
                        .accessibility_label(tr("knowledge_model_details"))
                        .tooltip(tr("knowledge_model_details"))
                        .disabled(locked)
                        .debug_selector(move || format!("model-library-details-{index}"))
                        .on_click(move |_, window, cx| {
                            super::details::open(owner.clone(), index, window, cx)
                        }),
                )
                .action(
                    Button::new(format!("model-library-install-{index}"))
                        .icon(
                            Icon::new(if installed {
                                gpui_kit::assets::IconName::Check
                            } else {
                                gpui_kit::assets::IconName::Download
                            })
                            .text_color(if installed {
                                cx.theme().muted_foreground
                            } else {
                                cx.theme().primary
                            }),
                        )
                        .accessibility_label(tr(if installed {
                            "knowledge_installed"
                        } else {
                            "knowledge_install"
                        }))
                        .tooltip(tr(if installed {
                            "knowledge_installed"
                        } else {
                            "knowledge_install"
                        }))
                        .loading(installing)
                        .disabled(downloading || installed || installing)
                        .debug_selector(move || format!("model-library-install-{index}"))
                        .on_click(cx.listener(move |library, _, window, cx| {
                            library.install(index, window, cx)
                        })),
                ),
            );
        }
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(controls)
            .child(results)
            .when(self.cursor.is_some(), |view| {
                view.child(
                    Button::new("model-library-more")
                        .label(tr("model_library_more"))
                        .disabled(locked)
                        .debug_selector(|| "model-library-more".into())
                        .on_click(
                            cx.listener(|library, _, window, cx| library.search(true, window, cx)),
                        ),
                )
            })
            .when(self.partial, |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("model_library_partial")),
                )
            })
    }
}
