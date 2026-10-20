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
};

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.section(self.kind, cx)
    }
}

impl Panel {
    fn section(&self, kind: Kind, cx: &mut Context<Self>) -> Div {
        let locked = self.locked() || self.settings.is_none();
        let (title, label) = match kind {
            Kind::Embedding => ("knowledge_settings", "knowledge_default_model"),
            Kind::Ocr => ("runtime_ocr", "runtime_ocr_model"),
            Kind::Speech => unreachable!("speech belongs to the controller"),
        };
        let mut controls = Group::new(title);
        if kind == Kind::Ocr {
            controls = controls.child(Row::new(
                "runtime_ocr_enable",
                Switch::new("runtime-ocr-enable")
                    .checked(
                        self.settings
                            .as_ref()
                            .is_none_or(|settings| settings.choice(kind).enabled),
                    )
                    .disabled(locked)
                    .accessibility_label(tr("runtime_ocr_enable"))
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
                    } else {
                        "runtime-ocr-model"
                    })
                    .w_full()
                    .disabled(locked)
                    .accessibility_label(tr(label)),
            )
            .wide(),
        );
        let tabs = TabBar::new("local-model-tabs")
            .segmented()
            .menu(false)
            .selected_index(self.tab)
            .child(Tab::new().label(tr("knowledge_installed")))
            .child(Tab::new().label(tr("knowledge_missing")))
            .on_click(cx.listener(|panel, tab, _, cx| {
                panel.tab = *tab;
                cx.notify();
            }));
        let entries = self
            .model_entries(kind)
            .into_iter()
            .filter(|model| (self.tab == 0) == matches!(model.status, ModelStatus::Installed))
            .collect();
        let rows = Group::new("local_models")
            .heading(false)
            .empty(
                IconName::Cpu,
                if self.tab == 0 {
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
        v_flex().gap_6().child(controls).child(
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
                            Button::new("local-model-library")
                                .ghost()
                                .small()
                                .icon(gpui_kit::assets::IconName::Download)
                                .tooltip(tr("model_library_manage"))
                                .accessibility_label(tr("model_library_manage"))
                                .disabled(locked)
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.library(kind, window, cx)
                                })),
                        ),
                )
                .child(rows),
        )
    }
}
