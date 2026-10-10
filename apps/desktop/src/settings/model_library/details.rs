use super::*;
use crate::settings::{
    entry::Entry,
    group::{Group, Row},
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::{DialogClose, DialogFooter},
    input::Input,
    select::Select,
};

struct Details {
    library: Entity<Library>,
}

pub(super) fn open(library: Entity<Library>, index: usize, window: &mut Window, cx: &mut App) {
    let state = library.read(cx);
    if state.loading || !state.available(cx) {
        return;
    }
    let Some(item) = state.items.get(index).cloned() else {
        return;
    };
    library.update(cx, |library, cx| {
        library.versions(item.source.repository, window, cx)
    });
    let details = cx.new(|cx| {
        cx.observe(&library, |_, _, cx| cx.notify()).detach();
        Details {
            library: library.clone(),
        }
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        let state = library.read(cx);
        let status = state.definition.as_ref().map(|definition| {
            state
                .target
                .model_status(state.draft(cx).as_ref().unwrap_or(definition), cx)
        });
        let installed = matches!(status, Some(ModelStatus::Installed));
        let installing = matches!(
            status,
            Some(ModelStatus::Downloading(_) | ModelStatus::Checking)
        );
        let locked = state.loading || !state.available(cx) || state.target.busy(cx);
        let download = library.clone();
        let close = library.clone();
        dialog
            .form_title(item.name.clone())
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .overlay_closable(false)
            .on_close(move |_, _, cx| {
                close.update(cx, |library, cx| {
                    library.generation += 1;
                    library.loading = false;
                    library.repository = None;
                    library.definition = None;
                    cx.notify();
                })
            })
            .child(details.clone())
            .footer(
                DialogFooter::new()
                    .gap_2()
                    .child(DialogClose::new().trigger(|button| {
                        button
                            .label(tr("settings_cancel"))
                            .debug_selector(|| "model-library-details-close".into())
                    }))
                    .child(
                        Button::new("model-library-download")
                            .primary()
                            .label(tr(if installed {
                                "knowledge_installed"
                            } else {
                                "knowledge_install"
                            }))
                            .loading(installing)
                            .disabled(
                                locked || installed || installing || state.definition.is_none(),
                            )
                            .debug_selector(|| "model-library-download".into())
                            .on_click(move |_, window, cx| {
                                download.update(cx, |library, cx| library.download(window, cx))
                            }),
                    ),
            )
    });
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let library = self.library.read(cx);
        let locked = library.loading || !library.available(cx) || library.target.busy(cx);
        let mut information = Group::new("knowledge_model_details")
            .heading(false)
            .child(
                Entry::new("model-library-detail-source", tr("model_library_source")).control(
                    div()
                        .whitespace_normal()
                        .child(library.repository.clone().unwrap_or_default()),
                ),
            )
            .child(
                Row::new(
                    "model_library_version",
                    Select::new(&library.version).w_full().disabled(locked),
                )
                .wide(),
            );
        if let Some(definition) = &library.definition {
            information = information
                .child(
                    Entry::new("model-library-detail-license", tr("settings_license"))
                        .control(definition.license.clone()),
                )
                .child(
                    Entry::new("model-library-detail-size", tr("knowledge_model_size")).control(
                        format!(
                            "{:.1} MiB",
                            definition
                                .assets
                                .iter()
                                .map(|asset| asset.bytes)
                                .sum::<u64>() as f64
                                / 1024.
                                / 1024.
                        ),
                    ),
                );
            if definition.format.kind() == Kind::Embedding {
                information = information
                    .child(
                        Row::new(
                            "model_library_query_prefix",
                            div()
                                .w_full()
                                .debug_selector(|| "model-library-query-prefix".into())
                                .child(
                                    Input::new(&library.query_prefix)
                                        .id("model-library-query-prefix")
                                        .w_full()
                                        .disabled(locked),
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
                                    Input::new(&library.document_prefix)
                                        .id("model-library-document-prefix")
                                        .w_full()
                                        .disabled(locked),
                                ),
                        )
                        .wide(),
                    );
            }
        }
        information
    }
}
