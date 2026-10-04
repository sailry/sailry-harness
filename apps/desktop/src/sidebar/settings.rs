//! Settings contributions share the existing sidebar groups and native rows.
use super::*;
use sailry_protocol::plugin::desktop::SettingsGroup;

enum Entry {
    Section(Section),
    Plugin(usize, crate::settings::extensions::NavigationEntry),
}

impl Entry {
    fn order(&self) -> u16 {
        match self {
            Self::Section(section) => section.navigation_order(),
            Self::Plugin(_, entry) => entry.placement.order,
        }
    }
}

impl Shell {
    pub(super) fn settings_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.settings.read(cx).section;
        let mut plugins: Vec<_> = self
            .settings
            .read(cx)
            .plugin_settings_entries(cx)
            .into_iter()
            .enumerate()
            .collect();
        let groups = [
            SettingsGroup::App,
            SettingsGroup::Ai,
            SettingsGroup::Tools,
            SettingsGroup::System,
        ]
        .into_iter()
        .zip(Section::GROUPS)
        .map(|(group, (label, sections))| {
            let mut entries: Vec<_> = sections.iter().copied().map(Entry::Section).collect();
            let mut index = 0;
            while index < plugins.len() {
                if plugins[index].1.placement.group == group {
                    let (id, entry) = plugins.remove(index);
                    entries.push(Entry::Plugin(id, entry));
                } else {
                    index += 1;
                }
            }
            entries.sort_by_key(Entry::order);
            v_flex()
                .gap_0p5()
                .mb_3()
                .child(Self::group_title(label, cx))
                .children(entries.into_iter().map(|entry| {
                    match entry {
                        Entry::Section(section) => self
                            .sidebar_row(
                                ("setting", section as usize),
                                Row::Setting(section),
                                selected == section,
                                cx,
                            )
                            .debug_selector(move || section.key().into())
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(if section == Section::Dictation {
                                        Icon::default().path("icons/reicon/microphone.svg")
                                    } else {
                                        Icon::new(section.icon())
                                    })
                                    .child(tr(section.key())),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.settings
                                    .update(cx, |settings, cx| settings.select(section, cx));
                                cx.notify();
                            })),
                        Entry::Plugin(index, entry) => {
                            let selected = self.settings.read(cx).selected_plugin_settings()
                                == Some(entry.name.as_str());
                            let selector = entry.name.clone();
                            self.sidebar_row(
                                ("plugin-setting", index),
                                Row::PluginSetting(index),
                                selected,
                                cx,
                            )
                            .debug_selector(move || format!("plugin-settings-{selector}"))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        entry
                                            .icon
                                            .as_ref()
                                            .map(crate::assets::icons::icon)
                                            .unwrap_or_else(|| Icon::new(IconName::Settings2)),
                                    )
                                    .child(entry.label),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.settings.update(cx, |settings, cx| {
                                        settings.open_plugin_settings(&entry.name, cx)
                                    });
                                    cx.notify();
                                },
                            ))
                        }
                    }
                }))
        })
        .collect::<Vec<_>>();
        v_flex()
            .size_full()
            .text_color(cx.theme().sidebar_foreground)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .debug_selector(|| "settings-navigation-viewport".into())
                    .child(
                        v_flex()
                            .id("settings-navigation")
                            .debug_selector(|| "settings-navigation".into())
                            .px_3()
                            .py_2()
                            .gap_0p5()
                            .overflow_y_scrollbar()
                            .children(groups),
                    ),
            )
    }
}
