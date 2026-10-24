//! Declared plugin pages join the existing core groups and Kit rows.
use super::*;
use crate::settings::extensions::navigation::{self, Item};

impl Shell {
    pub(super) fn settings_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.settings.read(cx).section;
        let plugin = self
            .settings
            .read(cx)
            .selected_plugin_settings()
            .map(str::to_owned);
        let pages = self.settings.read(cx).settings_pages(cx);
        let groups = Section::GROUPS
            .into_iter()
            .map(|(group, label, sections)| {
                v_flex()
                    .gap_0p5()
                    .mb_3()
                    .child(Self::group_title(label, cx))
                    .children(
                        navigation::group(group, sections, &pages)
                            .into_iter()
                            .enumerate()
                            .map(|(index, item)| {
                                let active = item.selected(selected, plugin.as_deref());
                                match item {
                                    Item::Core(section) => self
                                        .sidebar_row(
                                            ("setting", section as usize),
                                            Row::Setting(section),
                                            active,
                                            cx,
                                        )
                                        .accessibility_label(tr(section.key()))
                                        .debug_selector(move || section.key().into())
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .child(if section == Section::Dictation {
                                                    Icon::default()
                                                        .path("icons/reicon/microphone.svg")
                                                } else {
                                                    Icon::new(section.icon())
                                                })
                                                .child(tr(section.key())),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.clear_extension_details(cx);
                                            this.settings.update(cx, |settings, cx| {
                                                settings.select(section, cx)
                                            });
                                            cx.notify();
                                        }))
                                        .into_any_element(),
                                    Item::Plugin(entry) => {
                                        let label =
                                            entry.navigation.label(&rust_i18n::locale()).to_owned();
                                        let icon = entry
                                            .navigation
                                            .icon
                                            .as_ref()
                                            .map(crate::assets::icons::icon)
                                            .unwrap_or_else(|| Icon::new(IconName::Settings2));
                                        self.sidebar_row(
                                            SharedString::from(format!(
                                                "setting-plugin-{}",
                                                entry.package.name
                                            )),
                                            Row::PluginSetting(group, index),
                                            active,
                                            cx,
                                        )
                                        .accessibility_label(label.clone())
                                        .debug_selector({
                                            let name = entry.package.name.clone();
                                            move || format!("settings-plugin-{name}")
                                        })
                                        .child(h_flex().gap_2().child(icon).child(label))
                                        .on_click(cx.listener(move |shell, _, window, cx| {
                                            if !shell
                                                .settings
                                                .read(cx)
                                                .settings_pages(cx)
                                                .contains(&entry)
                                            {
                                                return;
                                            }
                                            shell.open_settings_panel(
                                                &crate::settings::extensions::modal::OpenPanel {
                                                    node: entry.node,
                                                    package: entry.package.clone(),
                                                },
                                                window,
                                                cx,
                                            );
                                        }))
                                        .into_any_element()
                                    }
                                }
                            }),
                    )
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
