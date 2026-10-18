//! Reuse directory brands from Settings and the welcome guide.
use super::super::presets;
use super::*;

impl Editor {
    pub(in crate::settings::providers) fn brands(
        entries: Option<&[sailry_protocol::conversation::presets::Entry]>,
    ) -> Vec<(String, SharedString, AnyElement)> {
        let entries = entries.map(<[_]>::to_vec).unwrap_or_else(presets::bundled);
        presets::groups(&entries)
            .into_iter()
            .map(|(key, entries)| {
                let first = &entries[0];
                (
                    key,
                    brand_label(first),
                    presets::render_icon(first, rems(2.)),
                )
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
    if entry.preset.brand.as_deref() == Some("opencode") {
        return "OpenCode".into();
    }
    // Native methods share a brand while retaining their distinct method labels.
    if matches!(
        entry.source,
        sailry_protocol::conversation::presets::Source::Builtin
    ) && let Some(brand) = entry.preset.brand.as_deref()
        && let Some(base) = presets::bundled()
            .iter()
            .find(|value| value.preset.id == brand)
    {
        return presets::label(&base.preset).into();
    }
    presets::label(&entry.preset).into()
}
