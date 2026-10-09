use super::*;
use crate::settings::{
    entry::Entry,
    group::{Group, Row},
};
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
        let controls = v_flex()
            .gap_3()
            .child(
                Row::new(
                    "model_library_source",
                    Select::new(&self.hub).w_full().disabled(locked),
                )
                .wide(),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
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
                            .label(tr("model_library_search"))
                            .debug_selector(|| "model-library-search".into())
                            .disabled(locked)
                            .on_click(cx.listener(|library, _, window, cx| {
                                library.search(false, window, cx)
                            })),
                    ),
            );
        let mut results = Group::new("model_library_results")
            .heading(false)
            .empty(IconName::Search, "model_library_empty");
        for (index, item) in self.items.iter().enumerate() {
            let repository = item.repository.clone();
            let selected = self.repository.as_ref() == Some(&repository);
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
                    Button::new(format!("model-library-versions-{index}"))
                        .label(tr(if selected {
                            "model_library_selected"
                        } else {
                            "model_library_versions"
                        }))
                        .disabled(locked || selected)
                        .debug_selector(move || format!("model-library-versions-{index}"))
                        .on_click(cx.listener(move |library, _, window, cx| {
                            library.versions(repository.clone(), window, cx)
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
            .when(self.repository.is_some(), |view| {
                view.child(
                    Row::new(
                        "model_library_version",
                        Select::new(&self.version).w_full().disabled(locked),
                    )
                    .wide(),
                )
            })
            .when_some(self.definition.as_ref(), |view, definition| {
                view.when(definition.format.kind() == Kind::Embedding, |view| {
                    view.child(
                        Row::new(
                            "model_library_query_prefix",
                            div()
                                .w_full()
                                .debug_selector(|| "model-library-query-prefix".into())
                                .child(
                                    Input::new(&self.query_prefix)
                                        .id("model-library-query-prefix")
                                        .w_full()
                                        .disabled(downloading),
                                ),
                        )
                        .wide(),
                    )
                    .child(
                        Row::new(
                            "model_library_document_prefix",
                            div()
                                .w_full()
                                .debug_selector(|| "model-library-document-prefix".into())
                                .child(
                                    Input::new(&self.document_prefix)
                                        .id("model-library-document-prefix")
                                        .w_full()
                                        .disabled(downloading),
                                ),
                        )
                        .wide(),
                    )
                })
                .child(
                    Entry::new(
                        "model-library-definition",
                        v_flex()
                            .gap_1()
                            .child(div().font_medium().child(definition.name.clone()))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} · {}",
                                        definition.source.repository, definition.source.revision
                                    )),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} · {:.1} MiB",
                                        definition.license,
                                        definition
                                            .assets
                                            .iter()
                                            .map(|asset| asset.bytes)
                                            .sum::<u64>()
                                            as f64
                                            / 1024.
                                            / 1024.
                                    )),
                            ),
                    )
                    .action(
                        Button::new("model-library-download")
                            .primary()
                            .label(tr("dictation_download_model"))
                            .disabled(downloading)
                            .debug_selector(|| "model-library-download".into())
                            .on_click(
                                cx.listener(|library, _, window, cx| library.download(window, cx)),
                            ),
                    ),
                )
            })
            .when(self.loading, |view| {
                view.child(
                    div()
                        .debug_selector(|| "model-library-loading".into())
                        .child(spinner::Spinner::new()),
                )
            })
            .child(Group::new("model_library_manage").children(self.target.rows(true, cx)))
    }
}
