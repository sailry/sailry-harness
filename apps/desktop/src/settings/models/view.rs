use super::*;
use crate::{
    settings::group::{Group, Row},
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    select::Select,
    switch::Switch,
    tab::{Tab, TabBar},
    tag::Tag,
};

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().gap_6().child(self.section(self.kind, cx))
    }
}

impl Panel {
    fn section(&self, kind: Kind, cx: &mut Context<Self>) -> Stateful<Div> {
        let locked = self.locked() || self.settings.is_none() || !self.available(kind);
        let tab = self.tabs.get(&kind).copied().unwrap_or_default();
        let (title, label, enabled, slug) = match kind {
            Kind::Embedding => (
                "knowledge_settings",
                "knowledge_default_model",
                "runtime_ocr_enable",
                "embedding",
            ),
            Kind::Ocr => (
                "runtime_ocr",
                "runtime_ocr_model",
                "runtime_ocr_enable",
                "ocr",
            ),
            Kind::Decision => (
                "runtime_decision",
                "runtime_decision_model",
                "runtime_decision_enable",
                "decision",
            ),
            Kind::Speech => unreachable!("speech belongs to the controller"),
        };
        let mut controls = Group::new(title);
        if matches!(kind, Kind::Ocr | Kind::Decision) {
            controls = controls.child(Row::new(
                enabled,
                Switch::new(SharedString::from(format!("local-model-{slug}-enable")))
                    .checked(
                        self.settings
                            .as_ref()
                            .is_none_or(|settings| settings.choice(kind).enabled),
                    )
                    .disabled(locked)
                    .accessibility_label(tr(enabled))
                    .on_click(cx.listener(move |panel, enabled, window, cx| {
                        if let Some(mut settings) = panel.settings.clone() {
                            let mut selection = settings.choice(kind);
                            selection.enabled = *enabled;
                            settings.choices.insert(kind, selection);
                            panel.submit(Command::SaveLocalModelSettings(settings), window, cx);
                        }
                    })),
            ));
        }
        controls = controls.child(
            Row::new(
                label,
                Select::new(&self.selectors[&kind])
                    .id(if kind == Kind::Embedding {
                        "knowledge-default-model"
                    } else if kind == Kind::Ocr {
                        "runtime-ocr-model"
                    } else {
                        "runtime-decision-model"
                    })
                    .w_full()
                    .disabled(locked)
                    .accessibility_label(tr(label)),
            )
            .wide(),
        );
        if matches!(
            self.availability.get(&kind),
            Some(Availability::Unavailable { .. })
        ) {
            controls = controls.child(Row::new(
                "local_models_status",
                Tag::secondary()
                    .small()
                    .child(tr("local_models_unavailable")),
            ));
        }
        let tabs = TabBar::new(SharedString::from(format!("local-model-{slug}-tabs")))
            .segmented()
            .menu(false)
            .selected_index(tab)
            .child(Tab::new().label(tr("knowledge_installed")))
            .child(Tab::new().label(tr("knowledge_missing")))
            .on_click(cx.listener(move |panel, tab, _, cx| {
                panel.tabs.insert(kind, *tab);
                cx.notify();
            }));
        let entries = self
            .model_entries(kind, cx)
            .into_iter()
            .filter(|model| (tab == 0) == matches!(model.status, ModelStatus::Installed))
            .collect();
        let rows = Group::new("local_models")
            .heading(false)
            .empty(
                IconName::Cpu,
                if tab == 0 {
                    "local_models_empty"
                } else {
                    "local_models_available_empty"
                },
            )
            .children(model_library::model_rows(
                model_library::Target::Node(cx.entity(), kind),
                entries,
                locked,
                false,
                cx,
            ));
        v_flex()
            .id(SharedString::from(format!("local-model-section-{slug}")))
            .gap_6()
            .child(controls)
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        h_flex()
                            .items_center()
                            .w_full()
                            .justify_between()
                            .gap_3()
                            .child(tabs)
                            .child(
                                Button::new(SharedString::from(format!(
                                    "local-model-{slug}-library"
                                )))
                                .ghost()
                                .small()
                                .icon(gpui_kit::assets::IconName::Download)
                                .tooltip(tr("model_library_manage"))
                                .accessibility_label(tr("model_library_manage"))
                                .disabled(locked)
                                .on_click(cx.listener(
                                    move |panel, _, window, cx| panel.library(kind, window, cx),
                                )),
                            ),
                    )
                    .child(rows),
            )
    }
}
