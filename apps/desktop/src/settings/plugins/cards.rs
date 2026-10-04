//! Plugin catalog composition using Kit buttons, switches, tabs and menus.
use super::*;
use crate::plugins::emblem;
use crate::settings::resource_card;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use sailry_protocol::plugin::Summary;

impl Workspace {
    pub(super) fn installed_plugins(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self
            .provider_link
            .as_ref()
            .expect("plugin inventory is bound");
        let catalog = &self.plugin_catalog;
        let busy = !live.connected || catalog.busy();
        let plugins = &catalog.packages;
        let owner = cx.entity();
        let download_owner = owner.clone();
        v_flex()
            .gap_4()
            .child(
                h_flex().justify_end().items_center().gap_3().child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("plugins-check-updates")
                                .label(tr("plugins_check_updates"))
                                .disabled(busy || plugins.is_empty())
                                .loading(catalog.updates.checking())
                                .debug_selector(|| "plugins-check-updates".into())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.check_plugin_updates(window, cx)
                                })),
                        )
                        .when(
                            catalog.updates.available() > 0 || catalog.updates.applying(),
                            |view| {
                                view.child(
                                    Button::new("plugins-update-all")
                                        .label(tr("plugins_update_all"))
                                        .disabled(busy)
                                        .loading(catalog.updates.applying())
                                        .debug_selector(|| "plugins-update-all".into())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.update_plugins(None, window, cx)
                                        })),
                                )
                            },
                        )
                        .child(
                            Button::new("plugins-download")
                                .label(tr("plugins_download"))
                                .disabled(busy)
                                .debug_selector(|| "plugins-download".into())
                                .on_click(move |_, window, cx| {
                                    download::open(download_owner.clone(), None, window, cx)
                                }),
                        )
                        .child(
                            Button::new("plugins-install")
                                .primary()
                                .disabled(busy)
                                .debug_selector(|| "plugins-install".into())
                                .label(tr("plugins_install"))
                                .on_click(move |_, window, cx| {
                                    dialog::open(owner.clone(), None, false, window, cx)
                                }),
                        ),
                ),
            )
            .when(catalog.retryable(), |view| {
                view.child(
                    Button::new("plugins-retry")
                        .label(tr("plugins_retry"))
                        .debug_selector(|| "plugins-retry".into())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.retry_plugin_action(window, cx)),
                        ),
                )
            })
            .when(plugins.is_empty() && live.connected, |view| {
                view.child(
                    Group::new("plugins_installed")
                        .heading(false)
                        .empty(IconName::Inbox, "plugins_none"),
                )
            })
            .child(
                resource_card::grid()
                    .debug_selector(|| "plugin-cards".into())
                    .children(
                        plugins
                            .iter()
                            .map(|plugin| self.plugin_card(plugin, busy, cx)),
                    ),
            )
            .into_any_element()
    }

    fn plugin_card(&self, plugin: &Summary, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let info = self
            .plugin_catalog
            .metadata
            .as_ref()
            .and_then(|state| state.read(cx).entries.get(&plugin.name));
        let title = info
            .map(metadata::title)
            .unwrap_or_else(|| plugin.name.clone());
        let description = info
            .and_then(metadata::description)
            .or_else(|| plugin.description.clone());
        let has_mcp = info.is_some_and(|info| !info.mcp.is_empty());
        let configured = has_mcp
            || info.is_some_and(|info| {
                info.extension
                    .as_ref()
                    .is_some_and(|ext| ext.settings_page.is_some())
                    || info
                        .settings
                        .as_ref()
                        .is_some_and(|schema| !schema.properties.is_empty())
            });
        let contributed = info.is_some_and(|info| {
            info.extension
                .as_ref()
                .is_some_and(|ext| ext.settings_page.is_some())
        });
        let update_ready = info.is_some_and(|info| info.summary == *plugin);
        let online = info.is_some_and(|info| {
            matches!(
                info.origin,
                Some(sailry_protocol::plugin::Origin::Online { .. })
            )
        });
        let available = self
            .plugin_catalog
            .updates
            .entries
            .get(&plugin.name)
            .and_then(|entry| entry.candidate.as_ref());
        let update_name = plugin.name.clone();
        let owner = cx.entity();
        let details = plugin.clone();
        let toggle = plugin.clone();
        let menu_owner = cx.entity();
        let menu_plugin = plugin.clone();
        let name = plugin.name.clone();
        let group = SharedString::from(format!("plugin-card-{name}"));
        let summary = resource_card::summary(
            format!("plugin-details-{name}"),
            title,
            description.unwrap_or_default(),
            div()
                .debug_selector({
                    let name = name.clone();
                    move || format!("plugin-icon-{name}")
                })
                .child(emblem::render(
                    &plugin.name,
                    info.and_then(emblem::glyph),
                    info.and_then(|info| info.icon.as_deref()),
                    px(48.),
                    cx,
                ))
                .into_any_element(),
            cx,
        )
        .disabled(
            !self
                .provider_link
                .as_ref()
                .is_some_and(|live| live.connected),
        )
        .on_click(move |_, window, cx| details::open(owner.clone(), details.clone(), window, cx));
        let actions = h_flex()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .when_some(available, |actions, candidate| {
                actions.child(
                    Button::new(SharedString::from(format!("plugin-update-{update_name}")))
                        .small()
                        .icon(IconName::ArrowDown)
                        .label(tr("plugins_update_available"))
                        .tooltip(
                            rust_i18n::t!(
                                "plugins_update_version",
                                version =
                                    candidate.info().summary.version.as_deref().unwrap_or("—")
                            )
                            .to_string(),
                        )
                        .disabled(busy)
                        .debug_selector({
                            let name = update_name.clone();
                            move || format!("plugin-update-{name}")
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.update_plugins(Some(&update_name), window, cx)
                        })),
                )
            })
            .child(
                resource_card::menu(format!("plugin-menu-{name}"), group)
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        let owner = menu_owner.clone();
                        let plugin = menu_plugin.clone();
                        let menu = menu.item(
                            PopupMenuItem::new(tr("plugins_settings_title"))
                                .icon(IconName::Settings2)
                                .disabled(busy || !configured)
                                .on_click(move |_, window, cx| {
                                    if has_mcp {
                                        details::open(owner.clone(), plugin.clone(), window, cx);
                                    } else if contributed {
                                        owner.update(cx, |owner, cx| {
                                            owner.open_plugin_settings(&plugin.name, cx)
                                        });
                                    } else {
                                        configuration::open(
                                            owner.clone(),
                                            plugin.clone(),
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                        );
                        let owner = menu_owner.clone();
                        let plugin = menu_plugin.clone();
                        let remove_owner = menu_owner.clone();
                        let remove = menu_plugin.clone();
                        menu.separator()
                            .item(
                                PopupMenuItem::new(tr(if online {
                                    "plugins_check_updates"
                                } else {
                                    "plugins_update"
                                }))
                                .icon(IconName::Redo)
                                .disabled(busy || !update_ready)
                                .on_click(
                                    move |_, window, cx| {
                                        if online {
                                            owner.update(cx, |owner, cx| {
                                                owner.check_plugin_updates(window, cx)
                                            });
                                        } else {
                                            dialog::open(
                                                owner.clone(),
                                                Some(plugin.clone()),
                                                false,
                                                window,
                                                cx,
                                            )
                                        }
                                    },
                                ),
                            )
                            .item(
                                PopupMenuItem::new(tr("plugins_uninstall"))
                                    .icon(IconName::CircleX)
                                    .disabled(busy)
                                    .on_click(move |_, window, cx| {
                                        dialog::open(
                                            remove_owner.clone(),
                                            Some(remove.clone()),
                                            true,
                                            window,
                                            cx,
                                        )
                                    }),
                            )
                    }),
            )
            .child(
                div()
                    .debug_selector({
                        let name = name.clone();
                        move || format!("plugin-toggle-{name}")
                    })
                    .child(
                        Switch::new(SharedString::from(format!("plugin-toggle-{name}")))
                            .disabled(busy)
                            .checked(plugin.enabled)
                            .accessibility_label(tr("plugins_enabled"))
                            .on_click(cx.listener(move |this, enabled, _, cx| {
                                cx.stop_propagation();
                                this.toggle_plugin(&toggle, *enabled, cx);
                            })),
                    ),
            );
        resource_card::card(format!("plugin-card-{name}"), summary, actions, cx)
    }
}
