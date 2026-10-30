use super::*;
use crate::{
    settings::group::{Group, Row},
    tr,
};
use gpui_kit::component::{
    button::Button,
    select::Select,
    switch::Switch,
    tab::{Tab, TabBar},
    tag::Tag,
};
use gpui_kit::prelude::FluentBuilder as _;

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
        let entries = self.model_entries(kind, cx);
        let catalog = self
            .catalogs
            .get(&kind)
            .filter(|_| tab == AVAILABLE)
            .cloned();
        let tabs = TabBar::new(SharedString::from(format!("local-model-{slug}-tabs")))
            .segmented()
            .menu(false)
            .selected_index(tab)
            .child(Tab::new().label(tr("knowledge_installed")))
            .child(Tab::new().label(tr("local_models_available")))
            .on_click(
                cx.listener(move |panel, tab, window, cx| panel.show(kind, *tab, window, cx)),
            );
        let header = h_flex()
            .w_full()
            .items_center()
            .flex_wrap()
            .gap_3()
            .child(tabs)
            .when_some(catalog.as_ref(), |header, catalog| {
                header.child(catalog.read(cx).controls(locked))
            });
        let body = match catalog {
            Some(catalog) => catalog.into_any_element(),
            None => Group::new("local_models")
                .heading(false)
                .empty(IconName::Cpu, "local_models_empty")
                .empty_action(
                    Button::new(SharedString::from(format!("local-model-{slug}-browse")))
                        .outline()
                        .small()
                        .label(tr("local_models_browse"))
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.show(kind, AVAILABLE, window, cx)
                        })),
                )
                .children(model_library::model_rows(
                    model_library::Target::Node(cx.entity(), kind),
                    entries
                        .into_iter()
                        .filter(|model| matches!(model.status, ModelStatus::Installed))
                        .collect(),
                    locked,
                    false,
                    cx,
                ))
                .into_any_element(),
        };
        v_flex()
            .id(SharedString::from(format!("local-model-section-{slug}")))
            .gap_6()
            .child(controls)
            .child(v_flex().gap_4().child(header).child(body))
    }
}
