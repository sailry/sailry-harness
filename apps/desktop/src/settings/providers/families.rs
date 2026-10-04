use super::*;
use gpui_kit::component::tab::{Tab, TabBar};

impl Workspace {
    pub(super) fn provider_families(&self, cx: &mut Context<Self>) -> AnyElement {
        let choices = ModelCategory::ALL;
        let selected = choices
            .iter()
            .position(|category| *category == self.providers.category)
            .unwrap_or(0);
        let header = h_flex()
            .debug_selector(|| "provider-families".into())
            .max_w_full()
            .min_w_0()
            .h_9()
            .child(
                TabBar::new("provider-families")
                    .segmented()
                    .max_w_full()
                    .min_w_0()
                    .menu(true)
                    .selected_index(selected)
                    .children(choices.iter().map(|category| {
                        let key = category.key();
                        Tab::new()
                            .debug_selector(move || key.into())
                            .icon(category.icon())
                            .label(tr(key))
                    }))
                    .on_click(cx.listener(move |this, index, _, cx| {
                        if let Some(category) = choices.get(*index) {
                            this.providers.category = *category;
                            cx.notify();
                        }
                    })),
            );
        header.into_any_element()
    }
}

#[cfg(test)]
mod tests;
