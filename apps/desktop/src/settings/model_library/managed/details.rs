use super::*;
use crate::{settings::group::Group, theme::DialogStyle as _};
use gpui_kit::component::{
    dialog::{DialogClose, DialogFooter},
    progress::Progress,
};

struct Details {
    target: Target,
    key: ModelKey,
}

pub(super) fn open(target: Target, key: ModelKey, window: &mut Window, cx: &mut App) {
    if target.entry(key, cx).is_none() {
        return;
    }
    let details = cx.new(|cx| {
        match &target {
            Target::Knowledge(owner) => {
                cx.observe(owner, |_, _, cx| cx.notify()).detach();
            }
            Target::Speech(owner) => {
                cx.observe(owner, |_, _, cx| cx.notify()).detach();
            }
        }
        Details { target, key }
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        let state = details.read(cx);
        let model = state.target.entry(state.key, cx);
        let mut footer = DialogFooter::new()
            .gap_2()
            .child(DialogClose::new().trigger(|button| {
                button
                    .label(tr("knowledge_close"))
                    .debug_selector(|| "model-details-close".into())
            }));
        if let Some(model) = &model {
            if matches!(model.status, ModelStatus::Downloading(_)) {
                let target = state.target.clone();
                let key = state.key;
                footer = footer.child(
                    Button::new("model-details-cancel")
                        .debug_selector(|| "model-details-cancel".into())
                        .label(tr("settings_cancel"))
                        .disabled(model.cancelling)
                        .on_click(move |_, window, cx| target.cancel(key, window, cx)),
                );
            } else if model.removable && matches!(model.status, ModelStatus::Installed) {
                let target = state.target.clone();
                let key = state.key;
                footer = footer.child(
                    Button::new("model-details-update")
                        .debug_selector(|| "model-details-update".into())
                        .label(tr("settings_update"))
                        .disabled(state.target.busy(cx))
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            target.update(key, window, cx);
                        }),
                );
            }
            if model.removable
                && !matches!(
                    model.status,
                    ModelStatus::Downloading(_) | ModelStatus::Checking
                )
            {
                let target = state.target.clone();
                let key = state.key;
                let name = model.name.clone();
                footer = footer.child(
                    Button::new("model-details-delete")
                        .debug_selector(|| "model-details-delete".into())
                        .label(tr("knowledge_delete"))
                        .disabled(state.target.busy(cx))
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            target.remove(key, name.clone(), window, cx);
                        }),
                );
            }
        }
        dialog
            .form_title(model.map_or_else(|| tr("knowledge_model_details"), |model| model.name))
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .child(details.clone())
            .footer(footer)
    });
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(model) = self.target.entry(self.key, cx) else {
            return v_flex();
        };
        let status_id = self.key.selector("status");
        let mut information =
            Group::new("knowledge_model_details")
                .heading(false)
                .child(Entry::new(
                    "model-detail-status",
                    div()
                        .debug_selector(move || status_id.clone())
                        .child(tr(label(model.status))),
                ));
        if let Some(definition) = self.target.definition(self.key, cx) {
            information = information
                .child(
                    Entry::new("model-detail-source", tr("model_library_source")).control(
                        div()
                            .whitespace_normal()
                            .child(definition.source.repository),
                    ),
                )
                .child(
                    Entry::new("model-detail-version", tr("model_library_version"))
                        .control(div().whitespace_normal().child(definition.source.revision)),
                )
                .child(
                    Entry::new("model-detail-license", tr("settings_license"))
                        .control(definition.license),
                )
                .child(
                    Entry::new("model-detail-size", tr("knowledge_model_size")).control(size(
                        definition.assets.iter().map(|asset| asset.bytes).sum(),
                    )),
                );
        }
        let mut view = v_flex().gap_4().child(information);
        if let ModelStatus::Downloading(progress) = model.status {
            let progress_id = self.key.selector("progress");
            let speed_id = self.key.selector("speed");
            view = view
                .child(
                    div()
                        .id(SharedString::from(progress_id.clone()))
                        .test_support()
                        .debug_selector(move || progress_id.clone())
                        .child(
                            Progress::new(SharedString::from(self.key.selector("progress")))
                                .value(progress.percent)
                                .small()
                                .w_full()
                                .accessibility_label(tr("knowledge_downloading")),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(speed_id.clone()))
                        .test_support()
                        .debug_selector(move || speed_id.clone())
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "{:.0}% · {}/s",
                            progress.percent,
                            size(progress.bytes_per_second as u64)
                        )),
                );
        }
        view
    }
}
