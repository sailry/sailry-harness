use super::*;
use crate::{
    settings::group::{Group, Row},
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    select::Select,
    tab::{Tab, TabBar},
};

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
            .segmented()
            .menu(false)
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
        let models = v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_3()
                    .debug_selector(|| "knowledge-model-toolbar".into())
                    .child(tabs)
                    .child(
                        Button::new("knowledge-model-library")
                            .ghost()
                            .small()
                            .icon(gpui_kit::assets::IconName::Download)
                            .accessibility_label(tr("model_library_manage"))
                            .debug_selector(|| "knowledge-model-library".into())
                            .disabled(locked)
                            .on_click(
                                cx.listener(|panel, _, window, cx| panel.library(window, cx)),
                            ),
                    ),
            )
            .child(
                Group::new("knowledge_model_list")
                    .heading(false)
                    .children(self.model_rows(cx.entity(), false, cx)),
            );
        v_flex().gap_6().child(controls).child(models)
    }
}
