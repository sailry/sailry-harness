use super::entry::Entry;
pub(super) mod authorization;
mod cards;
pub(super) mod configuration;
mod details;
pub(crate) use details::open_bound;
pub(super) mod dialog;
mod download;
mod editor;
pub(super) mod live;
mod market;
#[cfg(test)]
#[path = "plugins/tests/fixture.rs"]
pub(super) mod test_support;
mod updates;
use super::{Section, Workspace, draft, group::Group};
pub(super) use crate::plugins::metadata;
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    switch::Switch,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
pub(super) use live::Catalog;
pub(super) use metadata::Metadata;

#[derive(Clone)]
pub(super) struct Plugin {
    pub id: String,
    pub revision: usize,
    pub enabled: bool,
    pub package: String,
    pub directory: String,
    pub grants: [bool; 2],
    pub tools_expanded: bool,
    pub restarted: bool,
}

impl Plugin {
    pub fn example() -> Self {
        Self {
            id: "preview.toolkit".into(),
            revision: 1,
            enabled: true,
            package: "preview.sailry-plugin".into(),
            directory: String::new(),
            grants: [false; 2],
            tools_expanded: false,
            restarted: false,
        }
    }
}

impl Workspace {
    pub(super) fn plugin_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.provider_link.is_none() {
            return self.plugins(cx);
        }
        if self.plugin_catalog.market.is_none() {
            self.plugin_catalog.market = Some(market::create(
                cx.entity(),
                self.provider_link.as_ref().unwrap().binding.clone(),
                window,
                cx,
            ));
        }
        let owner = cx.entity();
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .justify_between()
                    .child(tr("plugins_installed"))
                    .child(
                        Button::new("plugins-manage")
                            .ghost()
                            .icon(IconName::Settings2)
                            .debug_selector(|| "plugins-manage".into())
                            .accessibility_label(tr("plugins_manage"))
                            .tooltip(tr("plugins_manage"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.select(Section::Plugins, cx);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .children(self.plugin_catalog.packages.iter().map(|plugin| {
                        let info = self
                            .plugin_catalog
                            .metadata
                            .as_ref()
                            .and_then(|state| state.read(cx).entries.get(&plugin.name));
                        let emblem = crate::plugins::emblem::render(
                            &plugin.name,
                            info.and_then(crate::plugins::emblem::glyph),
                            info.and_then(|info| info.icon.as_deref()),
                            px(48.),
                            cx,
                        );
                        let plugin = plugin.clone();
                        let owner = owner.clone();
                        let title = info
                            .map(metadata::title)
                            .unwrap_or_else(|| plugin.name.clone());
                        Button::new(SharedString::from(format!(
                            "installed-icon-{}",
                            plugin.name
                        )))
                        .ghost()
                        .h_auto()
                        .p_1()
                        .accessibility_label(title.clone())
                        .tooltip(title)
                        .debug_selector({
                            let name = plugin.name.clone();
                            move || format!("installed-icon-{name}")
                        })
                        .child(emblem)
                        .on_click(move |_, window, cx| {
                            details::open(owner.clone(), plugin.clone(), window, cx)
                        })
                    })),
            )
            .when(self.plugin_catalog.retryable(), |view| {
                view.child(
                    Button::new("plugins-retry")
                        .label(tr("plugins_retry"))
                        .debug_selector(|| "plugins-retry".into())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.retry_plugin_action(window, cx)),
                        ),
                )
            })
            .child(self.plugin_catalog.market.as_ref().unwrap().clone())
            .into_any_element()
    }

    pub(in crate::settings) fn plugin_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::component::tab::{Tab, TabBar};
        h_flex()
            .child(
                TabBar::new("plugins-navigation")
                    .segmented()
                    .selected_index(
                        Section::EXTENSIONS
                            .iter()
                            .position(|section| *section == self.section)
                            .unwrap_or(0),
                    )
                    .children(
                        Section::EXTENSIONS
                            .into_iter()
                            .zip([
                                "plugins-market-tab",
                                "plugins-manage-tab",
                                "plugins-skills-tab",
                                "plugins-mcp-tab",
                            ])
                            .map(|(section, selector)| {
                                Tab::new()
                                    .label(tr(section.key()))
                                    .debug_selector(move || selector.into())
                            }),
                    )
                    .on_click(cx.listener(|this, index, _, cx| {
                        if let Some(section) = Section::EXTENSIONS.get(*index) {
                            this.select(*section, cx);
                        }
                    })),
            )
            .into_any_element()
    }

    pub(super) fn plugins(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.provider_link.is_some() {
            return self.installed_plugins(cx);
        }
        let owner = cx.entity();
        let mut group = Group::new("plugins_installed").empty(IconName::Inbox, "plugins_none");
        for (index, plugin) in self.plugins.iter().enumerate() {
            let update = cx.entity();
            let configure = cx.entity();
            let remove = cx.entity();
            group = group.child(
                Entry::new(
                    format!("plugin-{index}"),
                    v_flex()
                        .gap_1()
                        .child(div().truncate().child(plugin.id.clone()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(
                                    rust_i18n::t!("plugins_revision", revision = plugin.revision)
                                        .to_string(),
                                ),
                        ),
                )
                .control(
                    Switch::new(("plugin-enabled", index))
                        .accessibility_label(tr("plugins_enabled"))
                        .checked(plugin.enabled)
                        .on_click(cx.listener(move |this, enabled, _, cx| {
                            this.plugins[index].enabled = *enabled;
                            cx.notify();
                        })),
                )
                .action(
                    Button::new(("plugin-configure", index))
                        .icon(IconName::Settings2)
                        .tooltip(tr("plugins_configure"))
                        .accessibility_label(tr("plugins_configure"))
                        .on_click(move |_, window, cx| {
                            editor::open(configure.clone(), Some(index), false, window, cx)
                        }),
                )
                .action(
                    Button::new(("plugin-update", index))
                        .icon(IconName::Redo)
                        .tooltip(tr("settings_update"))
                        .accessibility_label(tr("settings_update"))
                        .disabled(!plugin.enabled)
                        .on_click(move |_, window, cx| {
                            editor::open(update.clone(), Some(index), true, window, cx)
                        }),
                )
                .action(
                    Button::new(("plugin-uninstall", index))
                        .icon(IconName::CircleX)
                        .tooltip(tr("plugins_uninstall"))
                        .accessibility_label(tr("plugins_uninstall"))
                        .on_click(move |_, window, cx| {
                            let owner = remove.clone();
                            draft::confirm(
                                "plugins_uninstall",
                                move |cx| {
                                    owner.update(cx, |this, cx| {
                                        this.plugins.remove(index);
                                        cx.notify();
                                    })
                                },
                                window,
                                cx,
                            );
                        }),
                ),
            );
        }
        group = group.action(
            Button::new("plugins-install")
                .primary()
                .debug_selector(|| "plugins-install".into())
                .label(tr("plugins_install"))
                .on_click(move |_, window, cx| editor::open(owner.clone(), None, true, window, cx)),
        );
        v_flex().gap_4().child(group).into_any_element()
    }

    pub(super) fn mcp(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.provider_link.is_some() {
            return self.standalone_mcp(cx);
        }
        let mut group = Group::new("mcp_servers");
        for (index, plugin) in self.plugins.iter().enumerate() {
            group = group.child(
                v_flex()
                    .child(
                        Entry::new(
                            format!("mcp-{index}"),
                            v_flex()
                                .gap_1()
                                .child(div().truncate().child(plugin.id.clone()))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(tr(if !plugin.enabled {
                                            "mcp_stopped"
                                        } else if plugin.restarted {
                                            "mcp_restarted"
                                        } else {
                                            "mcp_ready"
                                        })),
                                ),
                        )
                        .action(
                            Button::new(("mcp-tools", index))
                                .icon(if plugin.tools_expanded {
                                    IconName::ChevronUp
                                } else {
                                    IconName::ChevronDown
                                })
                                .tooltip(tr(if plugin.tools_expanded {
                                    "mcp_hide_tools"
                                } else {
                                    "mcp_tools"
                                }))
                                .accessibility_label(tr("mcp_tools"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.plugins[index].tools_expanded =
                                        !this.plugins[index].tools_expanded;
                                    cx.notify();
                                })),
                        )
                        .action(
                            Button::new(("mcp-restart", index))
                                .icon(IconName::Redo)
                                .tooltip(tr("mcp_restart"))
                                .accessibility_label(tr("mcp_restart"))
                                .disabled(!plugin.enabled)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.plugins[index].restarted = true;
                                    cx.notify();
                                })),
                        ),
                    )
                    .when(plugin.tools_expanded, |row| {
                        row.child(div().pb_3().text_sm().child(tr("mcp_example_tools")))
                    }),
            );
        }
        group = group.action(
            Button::new("mcp-open-plugins")
                .primary()
                .debug_selector(|| "mcp-open-plugins".into())
                .label(tr("mcp_manage_plugins"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.select(Section::Plugins, cx);
                })),
        );
        v_flex().gap_4().child(group).into_any_element()
    }
}
