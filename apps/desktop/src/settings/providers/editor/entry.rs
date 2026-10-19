//! Reuse directory brands from Settings and the welcome guide.
use super::super::presets;
use super::*;

impl Editor {
    pub(in crate::settings::providers) fn brands(
        entries: Option<&[sailry_protocol::conversation::presets::Entry]>,
        icon: Rems,
    ) -> Vec<(String, SharedString, AnyElement)> {
        let entries = entries.map(<[_]>::to_vec).unwrap_or_else(presets::bundled);
        presets::groups(&entries)
            .into_iter()
            .map(|(key, entries)| {
                let first = &entries[0];
                (key, brand_label(first), presets::brand_icon(first, icon))
            })
            .collect()
    }

    pub(in crate::settings::providers) fn choose_brand(
        &mut self,
        brand: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation_blocked() || self.editing.is_some() {
            return;
        }
        let Some((key, choices)) = presets::groups(&self.directory)
            .into_iter()
            .find(|(key, _)| *key == brand)
        else {
            self.flow.defer(brand);
            return;
        };
        self.flow.select(&key, choices.len());
        if choices.len() == 1 {
            self.choose_entry(choices[0].clone(), window, cx);
        }
        cx.notify();
    }
}

pub(super) fn brand_label(entry: &sailry_protocol::conversation::presets::Entry) -> SharedString {
    presets::brand_label(entry).into()
}
