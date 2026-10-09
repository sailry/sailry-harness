use super::*;
use crate::{
    settings::group::{Group, Row},
    tr,
};
use gpui_kit::component::{
    button::Button,
    select::Select,
    tab::{Tab, TabBar},
};
use gpui_kit::prelude::FluentBuilder as _;

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.locked();
        let controls = Group::new("knowledge_settings").child(
            Row::new(
                "knowledge_default_model",
                Select::new(&self.model)
                    .id("knowledge-default-model")
                    .w_full()
                    .disabled(locked)
                    .accessibility_label(tr("knowledge_default_model")),
            )
            .wide(),
        );
        let tabs = TabBar::new("knowledge-model-tabs")
            .selected_index(self.tab)
            .child(
                Tab::new()
                    .debug_selector(|| "knowledge-tab-installed".into())
                    .label(tr("knowledge_installed")),
            )
            .child(
                Tab::new()
                    .debug_selector(|| "knowledge-tab-available".into())
                    .label(tr("knowledge_missing")),
            )
            .on_click(cx.listener(|panel, tab, _, cx| {
                panel.tab = *tab;
                cx.notify();
            }));
        let models = Group::new("knowledge_models")
            .action(
                Button::new("knowledge-model-library")
                    .icon(gpui_kit::assets::IconName::Download)
                    .accessibility_label(tr("model_library_manage"))
                    .debug_selector(|| "knowledge-model-library".into())
                    .disabled(locked)
                    .on_click(cx.listener(|panel, _, window, cx| panel.library(window, cx))),
            )
            .child(tabs)
            .children(self.model_rows(cx.entity(), false, cx));
        v_flex()
            .gap_6()
            .child(controls)
            .child(models)
            .when(self.loading, |view| {
                view.child(gpui_kit::component::spinner::Spinner::new())
            })
    }
}
