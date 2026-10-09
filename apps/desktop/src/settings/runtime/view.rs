use super::*;
use crate::{
    settings::group::{Group, Row},
    tr,
};
use gpui_kit::{
    component::{
        button::Button,
        input::Input,
        select::Select,
        switch::Switch,
        tab::{Tab, TabBar},
    },
    prelude::FluentBuilder as _,
};

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.locked();
        let python = Group::new("runtime_python").child(
            Row::new(
                "runtime_python_index",
                div()
                    .id("runtime-python-index")
                    .debug_selector(|| "runtime-python-index".into())
                    .child(Input::new(&self.index).disabled(locked || self.python.is_none())),
            )
            .wide(),
        );
        let ocr = Group::new("runtime_ocr")
            .child(Row::new(
                "runtime_ocr_enable",
                Switch::new("runtime-ocr-enable")
                    .checked(
                        self.settings
                            .as_ref()
                            .is_none_or(|settings| settings.enabled),
                    )
                    .disabled(locked || self.settings.is_none())
                    .accessibility_label(tr("runtime_ocr_enable"))
                    .on_click(cx.listener(|panel, enabled, window, cx| {
                        if let Some(mut settings) = panel.settings.clone() {
                            settings.enabled = *enabled;
                            panel.submit(Command::SaveOcrSettings(settings), window, cx);
                        }
                    })),
            ))
            .child(
                Row::new(
                    "runtime_ocr_model",
                    Select::new(&self.model)
                        .id("runtime-ocr-model")
                        .w_full()
                        .disabled(locked || self.settings.is_none()),
                )
                .wide(),
            );
        let tabs = TabBar::new("runtime-model-tabs")
            .selected_index(self.tab)
            .child(Tab::new().label(tr("knowledge_installed")))
            .child(Tab::new().label(tr("knowledge_missing")))
            .on_click(cx.listener(|panel, tab, _, cx| {
                panel.tab = *tab;
                cx.notify();
            }));
        let entries = self
            .model_entries(cx)
            .into_iter()
            .filter(|model| (self.tab == 0) == matches!(model.status, ModelStatus::Installed))
            .collect();
        let models = Group::new("runtime_ocr_models")
            .action(
                Button::new("runtime-model-library")
                    .icon(gpui_kit::assets::IconName::Download)
                    .accessibility_label(tr("model_library_manage"))
                    .disabled(locked)
                    .on_click(cx.listener(|panel, _, window, cx| panel.library(window, cx))),
            )
            .child(tabs)
            .children(model_library::model_rows(
                model_library::Target::Ocr(cx.entity()),
                entries,
                locked,
                false,
                cx,
            ));
        v_flex()
            .gap_6()
            .child(python)
            .child(ocr)
            .child(models)
            .when(self.loading, |view| view.child(spinner::Spinner::new()))
    }
}
