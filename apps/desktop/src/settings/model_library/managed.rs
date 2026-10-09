//! The two model owners share one Kit list and progress presentation.
use super::*;
use crate::settings::entry::Entry;
use gpui_kit::component::{button::Button, progress::Progress};
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
            let status = match model.status {
                ModelStatus::Missing => "knowledge_missing",
                ModelStatus::Downloading(_) => "knowledge_downloading",
                ModelStatus::Installed => "knowledge_installed",
                ModelStatus::Checking => "dictation_checking",
                ModelStatus::Unavailable => "dictation_unavailable",
                ModelStatus::Failed => "dictation_retry",
            };
            let status_id = selector("status");
            let mut content = v_flex()
                .min_w_0()
                .gap_1()
                .child(div().font_medium().child(model.name.clone()))
                .when(!model.detail.is_empty(), |content| {
                    content.child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(model.detail.clone()),
                    )
                })
                .child(
                    div()
                        .debug_selector(move || status_id.clone())
                        .text_color(cx.theme().muted_foreground)
                        .child(tr(status)),
                );
            if let ModelStatus::Downloading(progress) = model.status {
                let progress_id = selector("progress");
                let speed_id = selector("speed");
                content = content
                    .child(
                        div().debug_selector(move || progress_id.clone()).child(
                            Progress::new(SharedString::from(selector("progress")))
                                .value(progress.percent)
                                .small()
                                .w_full()
                                .accessibility_label(tr("knowledge_downloading")),
                        ),
                    )
                    .child(
                        div()
                            .debug_selector(move || speed_id.clone())
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{:.0}% · {}/s",
                                progress.percent,
                                size(progress.bytes_per_second as u64)
                            )),
                    );
            }
            let mut entry = Entry::new(selector("row"), content);
            match model.status {
                ModelStatus::Missing | ModelStatus::Failed => {
                    let owner = target.clone();
                    let id = selector("install");
                    entry = entry.action(
                        Button::new(SharedString::from(id.clone()))
                            .debug_selector(move || id.clone())
                            .label(tr(if matches!(model.status, ModelStatus::Failed) {
                                "dictation_retry"
                            } else {
                                "knowledge_install"
                            }))
                            .disabled(locked)
                            .on_click(move |_, window, cx| owner.install(key, window, cx)),
                    );
                }
                ModelStatus::Downloading(_) => {
                    let owner = target.clone();
                    let id = selector("cancel");
                    entry = entry.action(
                        Button::new(SharedString::from(id.clone()))
                            .debug_selector(move || id.clone())
                            .label(tr("settings_cancel"))
                            .disabled(model.cancelling)
                            .on_click(move |_, window, cx| owner.cancel(key, window, cx)),
                    );
                }
                _ => {}
            }
            if model.selectable {
                let owner = target.clone();
                let id = selector("select");
                entry = entry.action(
                    Button::new(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .label(tr(if model.selected {
                            "model_library_selected"
                        } else {
                            "model_library_select"
                        }))
                        .disabled(locked || model.selected)
                        .on_click(move |_, window, cx| owner.select(key, window, cx)),
                );
            }
            if model.removable && !matches!(model.status, ModelStatus::Downloading(_)) {
                let owner = target.clone();
                let name = model.name.clone();
                let id = selector("delete");
                entry = entry.action(
                    Button::new(SharedString::from(id.clone()))
                        .debug_selector(move || id.clone())
                        .icon(IconName::CircleX)
                        .accessibility_label(tr("knowledge_delete"))
                        .disabled(locked)
                        .on_click(move |_, window, cx| owner.remove(key, name.clone(), window, cx)),
                );
            }
            entry
        })
        .collect()
}

fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024. * 1024.))
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.)
    }
}
