//! Plugin catalog composition using Kit buttons, switches, tabs and menus.
use super::*;
use crate::plugins::emblem;
use crate::settings::resource_card;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use sailry_protocol::plugin::Summary;

impl Workspace {
    pub(super) fn inventory_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self
            .provider_link
            .as_ref()
            .expect("plugin inventory is bound");
        let catalog = &self.plugin_catalog;
        let updates = catalog.updates.as_ref().map(|state| state.read(cx));
        let busy = !live.connected || catalog.busy(cx);
        let plugins = self.installed_plugins(cx);
        let available = updates.map_or(0, |state| {
            plugins
                .iter()
                .filter(|plugin| state.has_candidate(&plugin.name))
                .count()
        });
        let metadata = catalog.metadata.as_ref().map(|metadata| metadata.read(cx));
        let settled = metadata.is_some_and(|metadata| metadata.settled());
        let failed = metadata.is_some_and(|metadata| !metadata.errors.is_empty());
        let preparations = catalog
            .python
            .preparations
            .iter()
            .filter(|preparation| {
                !plugins
                    .iter()
                    .any(|plugin| plugin.name == preparation.package)
            })
            .collect::<Vec<_>>();
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
                                .disabled(
                                    busy || plugins.is_empty()
                                        || updates.is_some_and(|state| state.busy()),
                                )
                                .loading(updates.is_some_and(|state| state.checking()))
                                .debug_selector(|| "plugins-check-updates".into())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.check_plugin_updates(window, cx)
                                })),
                        )
                        .when(
                            available > 0 || updates.is_some_and(|state| state.applying()),
                            |view| {
                                view.child(
                                    Button::new("plugins-update-all")
                                        .label(tr("plugins_update_all"))
                                        .disabled(busy || updates.is_some_and(|state| state.busy()))
                                        .loading(updates.is_some_and(|state| state.applying()))
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
            .when(catalog.retryable(cx), |view| {
                view.child(
                    crate::feedback::retry("plugins-retry")
                        .debug_selector(|| "plugins-retry".into())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.retry_plugin_action(window, cx)),
                        ),
                )
            })
            .child(self.plugin_action_retries(cx))
            .when(
                catalog.python.connected && !preparations.is_empty(),
                |view| {
                    view.child(v_flex().gap_2().children(preparations.into_iter().map(
                        |preparation| {
                            let selector = format!("plugin-preparing-{}", preparation.package);
                            h_flex()
                                .min_w_0()
                                .gap_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .debug_selector(move || selector.clone())
                                .child(spinner::Spinner::new().small())
                                .child(preparation.package.clone())
                                .child(preparation::label(preparation.stage))
                        },
                    )))
                },
            )
            .when(failed, |view| {
                view.child(
                    crate::feedback::retry("plugins-metadata-retry").on_click(cx.listener(
                        |owner, _, _, cx| {
                            if let Some(metadata) = &owner.plugin_catalog.metadata {
                                metadata.update(cx, |metadata, cx| metadata.refresh(cx));
                            }
                        },
                    )),
                )
            })
            .when(
                plugins.is_empty()
                    && live.connected
                    && live.snapshot.is_some()
                    && settled
                    && !failed,
                |view| {
                    view.child(
                        Group::new("plugins_installed")
                            .heading(false)
                            .empty(IconName::Inbox, "plugins_none"),
                    )
                },
            )
            .into_any_element()
    }

    pub(super) fn plugin_action_retries(&self, cx: &mut Context<Self>) -> AnyElement {
        let node = self
            .provider_link
            .as_ref()
            .map(|live| live.binding.client.target());
        v_flex()
            .gap_2()
            .children(self.plugin_catalog.retries().into_iter().map(|name| {
                h_flex().items_center().gap_2().child(name.clone()).child(
                    crate::feedback::retry(SharedString::from(format!(
                        "plugin-action-retry-{name}"
                    )))
                    .on_click(cx.listener(move |owner, _, window, cx| {
                        if node.is_some()
                            && owner
                                .provider_link
                                .as_ref()
                                .map(|live| live.binding.client.target())
                                == node
                        {
                            owner.send_plugin_action(&name, window, cx);
                        }
                    })),
                )
            }))
            .into_any_element()
    }

    pub(super) fn caption(&self, plugin: &Summary, cx: &App) -> (String, String) {
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
        let description = self
            .plugin_catalog
            .python
            .package(&plugin.name)
            .map(|preparation| preparation::label(preparation.stage).to_string())
            .or(description);
        (title, description.unwrap_or_default())
    }

    pub(super) fn plugin_card(
        &self,
        plugin: &Summary,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let info = self
            .plugin_catalog
            .metadata
            .as_ref()
            .and_then(|state| state.read(cx).entries.get(&plugin.name));
        let (title, description) = self.caption(plugin, cx);
        let node = self
            .provider_link
            .as_ref()
            .expect("plugin inventory is bound")
            .binding
            .client
            .target();
        let update_ready = info.is_some_and(|info| info.summary == *plugin);
        let configure_info = info
            .filter(|info| super::super::extensions::modal::configurable(info))
            .cloned();
        let configure_binding = self.provider_link.as_ref().map(|live| live.binding.clone());
        let online = info.is_some_and(|info| {
            matches!(
                info.origin,
                Some(
                    sailry_protocol::plugin::Origin::Online { .. }
                        | sailry_protocol::plugin::Origin::Registry { .. }
                )
            )
        });
        let imported = info.and_then(|info| match &info.skill {
            Some(sailry_protocol::plugin::skills::Provenance::Imported { imported }) => {
                Some(imported.kind)
            }
            _ => None,
        });
        let available = self
            .plugin_catalog
            .updates
            .as_ref()
            .map(|state| state.read(cx))
            .and_then(|state| state.entries.get(&plugin.name))
            .and_then(|entry| entry.candidate.as_ref());

        let update_busy = self
            .plugin_catalog
            .updates
            .as_ref()
            .is_some_and(|state| state.read(cx).busy());
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
            description,
            div()
                .debug_selector({
                    let name = name.clone();
                    move || format!("plugin-icon-{name}")
                })
                .child(emblem::grid(
                    &plugin.name,
                    info.and_then(|info| info.extension.as_ref())
                        .and_then(|extension| extension.tint),
                    info.and_then(emblem::glyph),
                    info.and_then(|info| info.icon.as_deref()),
                    px(48.),
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
        .on_click(move |_, window, cx| {
            if owner
                .read(cx)
                .provider_link
                .as_ref()
                .is_none_or(|live| live.binding.client.target() != node)
            {
                return;
            }
            details::open(
                owner.clone(),
                details.clone(),
                sailry_link::CancellationToken::new(),
                window,
                cx,
            )
        });
        let actions = h_flex()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .child(
                resource_card::menu(format!("plugin-menu-{name}"), group)
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        let configure = menu_owner.clone();
                        let configure_info = configure_info.clone();
                        let configure_binding = configure_binding.clone();
                        let view_owner = menu_owner.clone();
                        let view_plugin = menu_plugin.clone();
                        let menu = menu
                            .item(PopupMenuItem::new(tr("plugins_information")).on_click(
                                move |_, window, cx| {
                                    if view_owner
                                        .read(cx)
                                        .provider_link
                                        .as_ref()
                                        .is_none_or(|live| live.binding.client.target() != node)
                                    {
                                        return;
                                    }
                                    details::open(
                                        view_owner.clone(),
                                        view_plugin.clone(),
                                        sailry_link::CancellationToken::new(),
                                        window,
                                        cx,
                                    );
                                },
                            ))
                            .when_some(configure_info, |menu, info| {
                                menu.item(
                                    PopupMenuItem::new(tr("plugins_configure"))
                                        .icon(IconName::Settings2)
                                        .disabled(busy)
                                        .on_click(move |_, window, cx| {
                                            if configure.read(cx).provider_link.as_ref().is_none_or(
                                                |live| live.binding.client.target() != node,
                                            ) {
                                                return;
                                            }
                                            if let Some(binding) = &configure_binding {
                                                super::super::extensions::modal::open(
                                                    configure.clone(),
                                                    binding.clone(),
                                                    info.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }),
                                )
                                .separator()
                            });
                        let owner = menu_owner.clone();
                        let plugin = menu_plugin.clone();
                        let remove_owner = menu_owner.clone();
                        let remove = menu_plugin.clone();
                        menu.item(
                            PopupMenuItem::new(tr(if imported.is_some() {
                                "skill_source_reimport"
                            } else if online {
                                "plugins_check_updates"
                            } else {
                                "plugins_update"
                            }))
                            .icon(IconName::Redo)
                            .disabled(busy || !update_ready || (online && update_busy))
                            .on_click(move |_, window, cx| {
                                if owner
                                    .read(cx)
                                    .provider_link
                                    .as_ref()
                                    .is_none_or(|live| live.binding.client.target() != node)
                                {
                                    return;
                                }
                                if let Some(kind) = imported {
                                    owner.update(cx, |owner, cx| {
                                        owner.reimport_skill(plugin.clone(), kind, window, cx)
                                    });
                                } else if online {
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
                            }),
                        )
                        .item(
                            PopupMenuItem::new(tr("plugins_uninstall"))
                                .icon(IconName::CircleX)
                                .disabled(busy)
                                .on_click(move |_, window, cx| {
                                    if remove_owner
                                        .read(cx)
                                        .provider_link
                                        .as_ref()
                                        .is_none_or(|live| live.binding.client.target() != node)
                                    {
                                        return;
                                    }
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
            .when_some(available, |actions, candidate| {
                actions.child(
                    Button::new(SharedString::from(format!("plugin-update-{update_name}")))
                        .small()
                        .ghost()
                        .icon(
                            Icon::new(gpui_kit::assets::IconName::Download)
                                .text_color(cx.theme().success),
                        )
                        .accessibility_label(tr("plugins_update_available"))
                        .tooltip(
                            rust_i18n::t!(
                                "plugins_update_version",
                                version = candidate.version().unwrap_or("—")
                            )
                            .to_string(),
                        )
                        .disabled(busy || update_busy)
                        .debug_selector({
                            let name = update_name.clone();
                            move || format!("plugin-update-{name}")
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this
                                .provider_link
                                .as_ref()
                                .is_some_and(|live| live.binding.client.target() == node)
                            {
                                this.update_plugins(Some(&update_name), window, cx);
                            }
                        })),
                )
            })
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
                            .on_click(cx.listener(move |this, enabled, window, cx| {
                                cx.stop_propagation();
                                if this
                                    .provider_link
                                    .as_ref()
                                    .is_some_and(|live| live.binding.client.target() == node)
                                {
                                    this.toggle_plugin(&toggle, *enabled, window, cx);
                                }
                            })),
                    ),
            );
        resource_card::card(format!("plugin-card-{name}"), summary, actions, cx)
    }
}
