use super::data::Selection;
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use sailry_protocol::conversation::{
    Provider,
    presets::{Entry as PresetEntry, Source},
};
use std::collections::BTreeMap;

struct Choice {
    selection: Selection,
    label: String,
    entry: Option<PresetEntry>,
}

pub(super) fn selection(channel: &Channel, provider: Option<&Provider>) -> Selection {
    for_reference(
        provider.and_then(|provider| provider.preset.as_ref()),
        channel.preset,
        channel.directory.as_ref().filter(|entry| {
            provider.is_none_or(|provider| {
                provider.preset.as_ref().is_some_and(|reference| {
                    entry.source == reference.source && entry.preset.id == reference.id
                })
            })
        }),
    )
}

pub(super) fn for_reference(
    reference: Option<&sailry_protocol::conversation::presets::PresetRef>,
    kind: Preset,
    entry: Option<&PresetEntry>,
) -> Selection {
    if let Some(entry) = entry {
        return Selection::Group(presets::brand(entry));
    }
    if let Some(reference) = reference {
        let id = match &reference.source {
            Source::Builtin => reference.id.clone(),
            Source::Partner => format!("partner:{}", reference.id),
            Source::Package { name } => format!("package:{name}:{}", reference.id),
        };
        return Selection::Group(id);
    }
    Selection::Group(
        presets::for_kind(kind)
            .as_ref()
            .map_or_else(|| "unlisted".into(), presets::brand),
    )
}

fn choices(
    entries: &[PresetEntry],
    saved: &[Selection],
    selected: &Selection,
    known: &[PresetEntry],
) -> Vec<Choice> {
    let mut choices = vec![Choice {
        selection: Selection::All,
        label: tr("provider_all").to_string(),
        entry: None,
    }];
    choices.extend(
        presets::groups(entries)
            .into_iter()
            .map(|(key, entries)| Choice {
                selection: Selection::Group(key),
                label: presets::brand_label(&entries[0]),
                entry: Some(entries[0].clone()),
            }),
    );
    for selection in saved.iter().chain(std::iter::once(selected)) {
        if !choices.iter().any(|choice| &choice.selection == selection)
            && let Selection::Group(id) = selection
        {
            let entry = known
                .iter()
                .find(|entry| presets::brand(entry) == *id)
                .cloned();
            choices.push(Choice {
                selection: selection.clone(),
                label: entry.as_ref().map_or_else(
                    || id.rsplit(':').next().unwrap_or(id).to_owned(),
                    presets::brand_label,
                ),
                entry,
            });
        }
    }
    choices
}

impl Workspace {
    pub(super) fn provider_families(&self, cx: &mut Context<Self>) -> AnyElement {
        let empty = BTreeMap::new();
        let providers = self
            .provider_link
            .as_ref()
            .map_or(&empty, |live| &live.providers);
        let saved = self
            .providers
            .channels
            .iter()
            .map(|channel| selection(channel, providers.get(&channel.id)))
            .collect::<Vec<_>>();
        let entries = presets::available(
            self.provider_link
                .as_ref()
                .map(|live| live.directory.entries.as_slice()),
        );
        let known = self
            .providers
            .channels
            .iter()
            .filter_map(|channel| channel.directory.clone())
            .collect::<Vec<_>>();
        let choices = choices(&entries, &saved, &self.providers.category, &known);
        let selected = choices
            .iter()
            .position(|choice| choice.selection == self.providers.category)
            .unwrap_or(0);
        let label = choices[selected].label.clone();
        let select = cx.listener(move |this, selection: &Selection, _, cx| {
            this.providers.category = selection.clone();
            cx.notify();
        });
        let select = std::rc::Rc::new(select);
        Button::new("provider-families")
            .debug_selector(|| "provider-families".into())
            .accessibility_label(tr("provider_type"))
            .outline()
            .dropdown_caret(true)
            .label(label)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                choices.iter().enumerate().fold(
                    menu.scrollable(true).max_h(px(360.)),
                    |menu, (index, choice)| {
                        let select = select.clone();
                        let text = choice.label.clone();
                        let icon = choice.entry.clone();
                        menu.item(
                            PopupMenuItem::element(move |_, _| {
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(icon.as_ref().map_or_else(
                                        || ModelCategory::Other.icon().into_any_element(),
                                        |entry| presets::render_icon(entry, rems(1.)),
                                    ))
                                    .child(text.clone())
                            })
                            .checked(index == selected)
                            .on_click({
                                let selection = choice.selection.clone();
                                move |_, window, cx| select(&selection, window, cx)
                            }),
                        )
                    },
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
