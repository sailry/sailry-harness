//! Reuse picker choices from Settings and the welcome guide.
use super::super::{brands, presets};
use super::*;

impl Editor {
    pub(in crate::settings::providers) fn choose(
        &mut self,
        choice: brands::Choice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation_blocked() || self.editing.is_some() {
            return;
        }
        let Some((brand, methods)) = brands::resolve(&self.directory, &choice) else {
            self.flow.defer(choice);
            return;
        };
        self.flow.select(&brand, methods.len());
        if let [method] = methods.as_slice() {
            self.choose_entry(method.clone(), window, cx);
        }
        cx.notify();
    }
}

pub(super) fn brand_label(entry: &sailry_protocol::conversation::presets::Entry) -> SharedString {
    presets::brand_label(entry).into()
}
