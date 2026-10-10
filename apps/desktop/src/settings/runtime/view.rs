use super::*;
use crate::{
    settings::group::{Group, Row},
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Input,
    select::Select,
    switch::Switch,
    tab::{Tab, TabBar},
};

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.page {
            Page::Python => self.python(cx).into_any_element(),
            Page::Ocr => self.ocr(cx).into_any_element(),
        }
    }
}

impl Panel {
    fn python(&self, cx: &mut Context<Self>) -> Group {
        let locked = self.locked() || self.python.is_none();
        let group = Group::new("runtime_python").child(
            Row::new(
                "runtime_python_index",
                div()
                    .debug_selector(|| "runtime-python-source".into())
                    .w_full()
                    .min_w_0()
                    .child(
                        Select::new(&self.source)
                            .id("runtime-python-source")
                            .w_full()
                            .accessibility_label(tr("runtime_python_index"))
                            .disabled(locked),
                    ),
            )
            .wide(),
        );
        if !self.custom_index(cx) {
            return group;
        }
        group.child(
            Row::new(
                "runtime_python_custom_url",
                div()
                    .id("runtime-python-index")
                    .debug_selector(|| "runtime-python-index".into())
                    .w_full()
                    .min_w_0()
                    .child(
                        Input::new(&self.index)
                            .id("runtime-python-index-input")
                            .w_full()
                            .disabled(locked),
                    ),
            )
            .wide(),
        )
    }

    fn ocr(&self, cx: &mut Context<Self>) -> Div {
        let locked = self.locked();
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
            .segmented()
            .menu(false)
            .selected_index(self.tab)
            .child(
                Tab::new()
                    .debug_selector(|| "runtime-tab-installed".into())
                    .label(tr("knowledge_installed")),
            )
            .child(
                Tab::new()
                    .debug_selector(|| "runtime-tab-available".into())
                    .label(tr("knowledge_missing")),
            )
            .on_click(cx.listener(|panel, tab, _, cx| {
                panel.tab = *tab;
                cx.notify();
            }));
        let entries = self
            .model_entries(cx)
            .into_iter()
            .filter(|model| (self.tab == 0) == matches!(model.status, ModelStatus::Installed))
            .collect();
        let rows = Group::new("runtime_ocr_list")
            .heading(false)
            .empty(
                IconName::Cpu,
                if self.tab == 0 {
                    "runtime_ocr_empty"
                } else {
                    "runtime_ocr_available_empty"
                },
            )
            .children(model_library::model_rows(
                model_library::Target::Ocr(cx.entity()),
                entries,
                locked,
                false,
                cx,
            ));
        let models = v_flex().gap_4().child(
            h_flex()
                .debug_selector(|| "runtime-model-toolbar".into())
                .items_center()
                .w_full()
                .justify_between()
                .gap_3()
                .child(tabs)
                .child(
                    Button::new("runtime-model-library")
                        .ghost()
                        .small()
                        .icon(gpui_kit::assets::IconName::Download)
                        .tooltip(tr("model_library_manage"))
                        .debug_selector(|| "runtime-model-library".into())
                        .accessibility_label(tr("model_library_manage"))
                        .disabled(locked)
                        .on_click(cx.listener(|panel, _, window, cx| panel.library(window, cx))),
                ),
        );
        let models = models.child(rows);
        v_flex().gap_6().child(ocr).child(models)
    }
}
