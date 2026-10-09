use super::*;
use crate::{
    settings::{
        entry::Entry,
        group::{Group, Row},
    },
    tr,
};
use gpui_kit::{
    component::{button::Button, input::Input, progress::Progress, select::Select},
    prelude::FluentBuilder,
};
use sailry_protocol::{
    Command,
    knowledge::{Owner, Source, Status},
};

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.locked();
        let controls = Group::new("knowledge_settings")
            .child(
                Row::new(
                    "knowledge_default_model",
                    div()
                        .w_full()
                        .debug_selector(|| "knowledge-default-model".into())
                        .child(
                            Select::new(&self.model)
                                .id("knowledge-default-model")
                                .w_full()
                                .disabled(locked)
                                .accessibility_label(tr("knowledge_default_model")),
                        ),
                )
                .wide(),
            )
            .child(
                Row::new(
                    "knowledge_mirror",
                    div()
                        .w_full()
                        .debug_selector(|| "knowledge-mirror".into())
                        .child(
                            Input::new(&self.mirror)
                                .id("knowledge-mirror")
                                .w_full()
                                .disabled(locked),
                        ),
                )
                .wide(),
            )
            .action(
                Button::new("knowledge-settings-save")
                    .debug_selector(|| "knowledge-settings-save".into())
                    .label(tr("settings_save"))
                    .disabled(locked)
                    .on_click(cx.listener(|panel, _, window, cx| panel.save(window, cx))),
            );
        let mut models = Group::new("knowledge_models").action(
            Button::new("knowledge-model-library")
                .label(tr("model_library_manage"))
                .debug_selector(|| "knowledge-model-library".into())
                .disabled(locked)
                .on_click(cx.listener(|panel, _, window, cx| panel.library(window, cx))),
        );
        models = models.children(self.model_rows(cx.entity(), false, cx));
        let owner = cx.entity();
        let mut collections = Group::new("knowledge_collections")
            .empty(IconName::Inbox, "knowledge_collections_empty")
            .action(
                Button::new("knowledge-add")
                    .debug_selector(|| "knowledge-add".into())
                    .icon(IconName::Plus)
                    .label(tr("knowledge_add"))
                    .disabled(locked)
                    .on_click(move |_, window, cx| {
                        editor::open(owner.clone(), None, window, cx);
                    }),
            );
        for collection in &self.collections {
            let id = collection.id;
            let source = match &collection.definition.source {
                Source::Worktree { path, .. }
                | Source::Directory { path }
                | Source::File { path } => path.clone(),
                Source::Storage { prefix, .. } => prefix.clone(),
            };
            let owner_label = match &collection.owner {
                Owner::Controller => tr("knowledge_owner_controller").to_string(),
                Owner::Plugin { name } => name.clone(),
            };
            let progress = &collection.progress;
            let status = tr(match progress.status {
                Status::Pending => "knowledge_pending",
                Status::Indexing => "knowledge_indexing",
                Status::Ready => "knowledge_ready",
                Status::Paused => "knowledge_paused",
            });
            let counts = rust_i18n::t!(
                "knowledge_counts",
                sources = progress.sources,
                chunks = progress.chunks
            );
            let mut content = v_flex()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .font_medium()
                        .child(collection.definition.name.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{owner_label} · {source}")),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{counts} · {status}")),
                );
            if progress.skipped > 0 {
                content = content.child(div().text_color(cx.theme().muted_foreground).child(
                    rust_i18n::t!("knowledge_skipped", count = progress.skipped).to_string(),
                ));
            }
            if progress.status == Status::Indexing {
                let percent = if progress.sources == 0 {
                    0.
                } else {
                    (progress.completed as f64 / progress.sources as f64 * 100.) as f32
                };
                content = content.child(
                    Progress::new(SharedString::from(format!("knowledge-index-{id}")))
                        .value(percent)
                        .loading(progress.sources == 0)
                        .small()
                        .w_full()
                        .accessibility_label(tr("knowledge_indexing")),
                );
            }
            let owner = cx.entity();
            let edit = collection.clone();
            let remove = collection.clone();
            let revision = collection.revision;
            let paused = collection.paused;
            collections = collections.child(
                Entry::new(format!("knowledge-collection-{id}"), content)
                    .action(
                        Button::new(SharedString::from(format!("knowledge-edit-{id}")))
                            .debug_selector(move || format!("knowledge-edit-{id}"))
                            .icon(IconName::Settings2)
                            .accessibility_label(tr("settings_edit"))
                            .disabled(locked)
                            .on_click(move |_, window, cx| {
                                editor::open(owner.clone(), Some(edit.clone()), window, cx);
                            }),
                    )
                    .action(
                        Button::new(SharedString::from(format!("knowledge-pause-{id}")))
                            .debug_selector(move || format!("knowledge-pause-{id}"))
                            .icon(if paused {
                                IconName::Play
                            } else {
                                IconName::Pause
                            })
                            .accessibility_label(tr(if paused {
                                "knowledge_resume"
                            } else {
                                "knowledge_pause"
                            }))
                            .disabled(locked)
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.submit(
                                    Command::PauseKnowledgeCollection {
                                        id,
                                        expected_revision: revision,
                                        paused: !paused,
                                    },
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .action(
                        Button::new(SharedString::from(format!("knowledge-rebuild-{id}")))
                            .debug_selector(move || format!("knowledge-rebuild-{id}"))
                            .icon(IconName::RotateCw)
                            .accessibility_label(tr("knowledge_rebuild"))
                            .disabled(locked || paused || progress.status == Status::Indexing)
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.submit(
                                    Command::RebuildKnowledgeCollection {
                                        id,
                                        expected_revision: revision,
                                    },
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .action(
                        Button::new(SharedString::from(format!("knowledge-delete-{id}")))
                            .debug_selector(move || format!("knowledge-delete-{id}"))
                            .icon(IconName::CircleX)
                            .accessibility_label(tr("knowledge_delete"))
                            .disabled(locked)
                            .on_click({
                                let owner = cx.entity();
                                move |_, window, cx| {
                                    consent::collection(owner.clone(), remove.clone(), window, cx)
                                }
                            }),
                    ),
            );
        }
        v_flex()
            .gap_6()
            .child(controls)
            .child(models)
            .child(collections)
            .when(self.loading, |view| {
                view.child(gpui_kit::component::spinner::Spinner::new())
            })
            .child(
                Button::new("knowledge-refresh")
                    .debug_selector(|| "knowledge-refresh".into())
                    .label(tr("knowledge_reload"))
                    .disabled(self.loading || self.pending)
                    .on_click(cx.listener(|panel, _, window, cx| panel.load(window, cx))),
            )
    }
}
