//! Model owners share the same Kit rows and detailed download progress.
mod details;
use super::*;
use crate::settings::entry::Entry;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder as _;

impl ModelKey {
    fn selector(self, action: &str) -> String {
        match self {
            Self::Embedding(model) => match action {
                "row" => format!("knowledge-model-{}", model.key()),
                "delete" => format!("knowledge-delete-model-{}", model.key()),
                action => format!("knowledge-{action}-{}", model.key()),
            },
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
            let key = model.key;
            let selector = move |action| {
                let value = key.selector(action);
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
            let id = selector("details");
            entry = entry.action(
                Button::new(SharedString::from(id.clone()))
                    .debug_selector(move || id.clone())
                    .icon(IconName::Settings2)
                    .accessibility_label(tr("knowledge_model_details"))
                    .on_click(move |_, window, cx| details::open(owner.clone(), key, window, cx)),
            );
            match model.status {
                ModelStatus::Missing | ModelStatus::Failed => {
                    let owner = target.clone();
                    let id = selector("install");
                    entry = entry.action(
                        Button::new(SharedString::from(id.clone()))
                            .debug_selector(move || id.clone())
                            .icon(gpui_kit::assets::IconName::Download)
                            .accessibility_label(tr("knowledge_install"))
                            .disabled(locked)
                            .on_click(move |_, window, cx| owner.install(key, window, cx)),
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
                        let id = selector("cancel");
                        entry = entry.action(
                            Button::new(SharedString::from(id.clone()))
                                .debug_selector(move || id.clone())
                                .icon(IconName::CircleX)
                                .accessibility_label(tr("settings_cancel"))
                                .disabled(model.cancelling)
                                .on_click(move |_, window, cx| owner.cancel(key, window, cx)),
                        );
                    }
                }
                _ => {}
            }
            if model.selectable {
                let owner = target.clone();
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
                        .on_click(move |_, window, cx| owner.select(key, window, cx)),
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
    fn entry(&self, key: ModelKey, cx: &App) -> Option<ManagedModel> {
        let entries = match self {
            Self::Knowledge(panel) => panel.read(cx).model_entries(cx),
            Self::Speech(panel) => panel.read(cx).model_entries(cx),
        };
        entries.into_iter().find(|model| match (model.key, key) {
            (ModelKey::Embedding(left), ModelKey::Embedding(right)) => left == right,
            (ModelKey::Speech(left), ModelKey::Speech(right)) => left == right,
            _ => false,
        })
    }
    fn definition(&self, key: ModelKey, cx: &App) -> Option<Definition> {
        match (self, key) {
            (Self::Knowledge(panel), ModelKey::Embedding(model)) => {
                panel.read(cx).definition_for(model)
            }
            (Self::Speech(panel), ModelKey::Speech(id)) => panel.read(cx).definition_for(id),
            _ => None,
        }
    }
}
