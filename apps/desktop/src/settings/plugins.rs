use super::entry::Entry;
mod cards;
pub(in crate::settings) mod classification;
pub(super) mod configuration;
pub(in crate::settings) mod details;
pub(super) mod dialog;
pub(in crate::settings) mod download;
mod editor;
mod inventory;
pub(super) mod live;
use super::market;
pub(in crate::settings) mod preparation;
pub(in crate::settings) mod updates;
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
            return Group::new("plugins_market")
                .empty(IconName::Search, "plugins_none")
                .into_any_element();
        }
        if self.plugin_catalog.market.is_none() {
            self.plugin_catalog.market = Some(market::create(
                cx.entity(),
                self.provider_link.as_ref().unwrap().binding.clone(),
                self.theme_updates.clone(),
                window,
                cx,
            ));
        }
        v_flex()
            .gap_4()
            .when(self.plugin_catalog.retryable(cx), |view| {
                view.child(
                    crate::feedback::retry("plugins-retry")
                        .debug_selector(|| "plugins-retry".into())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.retry_plugin_action(window, cx)),
                        ),
                )
            })
            .child(self.plugin_action_retries(cx))
            .child(self.plugin_catalog.market.as_ref().unwrap().clone())
            .into_any_element()
    }

    pub(super) fn plugins(&self, cx: &mut Context<Self>) -> AnyElement {
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
}
