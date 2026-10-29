//! Model owners share the same Kit rows and detailed download progress.
mod details;
use super::*;
use crate::settings::entry::Entry;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder as _;

impl ModelKey {
    fn selector(&self, action: &str) -> String {
        match self {
            Self::Node(kind, id) => {
                let prefix = match kind {
                    Kind::Embedding => "knowledge",
                    Kind::Ocr => "ocr",
                    Kind::Decision => "decision",
                    Kind::Speech => unreachable!("speech belongs to the controller"),
                };
                format!("{prefix}-{action}-{id}")
            }
            Self::Speech(Some(id)) => match action {
                "row" => format!("speech-model-{id}"),
                action => format!("speech-{action}-{id}"),
            },
            Self::Speech(None) => match action {
                "row" => "speech-current-model".into(),
                "install" => "dictation-download".into(),
                "cancel" => "dictation-download-cancel".into(),
                "select" => "speech-default-select".into(),
                "progress" => "dictation-download-progress".into(),
                "speed" => "dictation-download-speed".into(),
                "status" => "dictation-model-status".into(),
                "details" => "dictation-model-details".into(),
                _ => unreachable!("recommended speech model has no removal action"),
            },
        }
    }
}

pub(in crate::settings) fn rows(
    target: Target,
    models: Vec<ManagedModel>,
    locked: bool,
    library: bool,
    cx: &App,
) -> Vec<Entry> {
    models
        .into_iter()
        .map(|model| {
            let selected_key = model.key.clone();
            let selector = move |action| {
                let value = selected_key.selector(action);
                if library {
                    format!("model-library-{value}")
                } else {
                    value
                }
            };
            let content = v_flex()
                .min_w_0()
                .gap_1()
                .child(div().font_medium().child(model.name.clone()))
                .when(!model.detail.is_empty(), |content| {
                    content.child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(model.detail.clone()),
                    )
                });
            let mut entry = Entry::new(selector("row"), content);
            let owner = target.clone();
            let key = model.key.clone();
            let id = selector("details");
            entry = entry.action(
                Button::new(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .icon(IconName::Settings2)
                    .accessibility_label(tr("knowledge_model_details"))
                    .on_click(move |_, window, cx| {
                        details::open(owner.clone(), key.clone(), window, cx)
                    }),
            );
            match model.status {
                ModelStatus::Missing | ModelStatus::Failed => {
                    let owner = target.clone();
                    let key = model.key.clone();
                    let id = selector("install");
                    entry = entry.action(
                        Button::new(SharedString::from(id.clone()))
                            .debug_selector(move || id.clone())
                            .icon(gpui_kit::assets::IconName::Download)
                            .accessibility_label(tr("knowledge_install"))
                            .disabled(locked)
                            .on_click(move |_, window, cx| owner.install(key.clone(), window, cx)),
                    );
                }
                ModelStatus::Downloading(_) | ModelStatus::Checking => {
                    entry = entry.action(
                        Button::new(SharedString::from(selector("install")))
                            .icon(gpui_kit::assets::IconName::Download)
                            .loading(true)
                            .disabled(true)
                            .accessibility_label(tr("knowledge_downloading")),
                    );
                    if matches!(model.status, ModelStatus::Downloading(_)) {
                        let owner = target.clone();
                        let key = model.key.clone();
                        let id = selector("cancel");
                        entry = entry.action(
                            Button::new(SharedString::from(id.clone()))
                                .debug_selector(move || id.clone())
                                .icon(IconName::CircleX)
                                .accessibility_label(tr("settings_cancel"))
                                .disabled(model.cancelling)
                                .on_click(move |_, window, cx| {
                                    owner.cancel(key.clone(), window, cx)
                                }),
                        );
                    }
                }
                _ => {}
            }
            if model.selectable {
                let owner = target.clone();
                let key = model.key.clone();
                let id = selector("select");
                entry = entry.action(
                    Button::new(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .icon(IconName::Check)
                        .accessibility_label(tr(if model.selected {
                            "model_library_selected"
                        } else {
                            "model_library_select"
                        }))
                        .disabled(locked || model.selected)
                        .on_click(move |_, window, cx| owner.select(key.clone(), window, cx)),
                );
            }
            entry
        })
        .collect()
}

fn label(status: ModelStatus) -> &'static str {
    match status {
        ModelStatus::Missing => "knowledge_missing",
        ModelStatus::Downloading(_) => "knowledge_downloading",
        ModelStatus::Installed => "knowledge_installed",
        ModelStatus::Checking => "dictation_checking",
        ModelStatus::Unavailable => "dictation_unavailable",
        ModelStatus::Failed => "dictation_retry",
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024. * 1024.))
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.)
    }
}

impl Target {
    pub(super) fn entries(&self, cx: &App) -> Vec<ManagedModel> {
        match self {
            Self::Node(panel, kind) => panel.read(cx).model_entries(*kind, cx),
            Self::Speech(panel) => panel.read(cx).model_entries(cx),
        }
    }
    fn entry(&self, key: &ModelKey, cx: &App) -> Option<ManagedModel> {
        self.entries(cx).into_iter().find(|model| &model.key == key)
    }
    /// Catalog results only identify Node models by source; speech rows keep their own entries.
    pub(super) fn source_entry(&self, source: &Source, cx: &App) -> Option<ManagedModel> {
        match self {
            Self::Node(panel, kind) => panel.read(cx).source_entry(source, *kind, cx),
            Self::Speech(_) => None,
        }
    }
    fn definition(&self, key: &ModelKey, cx: &App) -> Option<Definition> {
        match (self, key) {
            (Self::Node(panel, kind), ModelKey::Node(owner, id)) if kind == owner => {
                panel.read(cx).definition_for(id)
            }
            (Self::Speech(panel), ModelKey::Speech(id)) => panel.read(cx).definition_for(*id),
            _ => None,
        }
    }
}
