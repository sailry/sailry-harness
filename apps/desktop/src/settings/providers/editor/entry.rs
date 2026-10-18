//! Reuse the same brand choices from Settings and the welcome guide.
use super::*;

impl Editor {
    pub(in crate::settings::providers) fn brands() -> Vec<(&'static str, Icon)> {
        kind::groups()
            .into_iter()
            .map(|(key, presets)| (key, presets[0].category().icon()))
            .collect()
    }

    pub(in crate::settings::providers) fn choose_brand(
        &mut self,
        brand: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation_blocked() || self.editing.is_some() {
            return;
        }
        let Some((key, choices)) = kind::groups().into_iter().find(|(key, _)| *key == brand) else {
            return;
        };
        self.flow.select(key, choices.len());
        if choices.len() == 1 {
            self.choose(choices[0], window, cx);
        }
        cx.notify();
    }
}
