//! Manifest-contributed settings share navigation and captured Node ownership.
use super::{
    Section, Workspace,
    group::{Group, Row},
};
use crate::{plugins::Panel, tr};
use gpui_kit::{
    component::{switch::Switch, *},
    prelude::FluentBuilder,
    *,
};
use sailry_protocol::plugin::{Info, desktop};

pub(crate) struct NavigationEntry {
    pub name: String,
    pub label: String,
    pub icon: Option<desktop::Icon>,
    pub placement: desktop::SettingsPlacement,
}

impl NavigationEntry {
    fn new(name: String, page: &desktop::Settings) -> Self {
        Self {
            name,
            label: page.navigation.label(&rust_i18n::locale()).into(),
            icon: page.navigation.icon.clone(),
            placement: page.placement,
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    pub selected: Option<String>,
    panel: Option<Entity<Panel>>,
    form: Option<Entity<super::plugins::configuration::Editor>>,
}

impl State {
    pub(super) fn reset(selected: Option<String>) -> Self {
        Self {
            selected,
            ..Default::default()
        }
    }

    pub(super) fn release(&mut self) {
        self.panel = None;
        self.form = None;
    }
}

impl Workspace {
    #[cfg(test)]
    pub(crate) fn plugin_settings_panel(&self) -> Option<Entity<Panel>> {
        self.extensions.panel.clone()
    }

    pub(crate) fn configure_panel(
        binding: crate::conversation::live::Binding,
        expected: sailry_protocol::plugin::Summary,
        window: &mut Window,
        cx: &mut App,
    ) {
        super::plugins::configuration::panel(
            super::providers::Binding {
                client: binding.client,
                runtime: binding.runtime,
                label: binding.host,
            },
            expected,
            window,
            cx,
        );
    }

    pub(crate) fn header(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.section.is_extension() {
            return Some(self.plugin_tabs(cx));
        }
        self.extensions
            .panel
            .as_ref()
            .and_then(|panel| panel.read(cx).header())
            .filter(|header| !header.read(cx).is_empty())
            .map(IntoElement::into_any_element)
    }

    pub(crate) fn plugin_settings_entries(&self, cx: &App) -> Vec<NavigationEntry> {
        if let Some(metadata) = &self.plugin_catalog.metadata {
            return metadata
                .read(cx)
                .entries
                .values()
                .filter_map(|info| {
                    let page = info.extension.as_ref()?.settings_page.as_ref()?;
                    Some(NavigationEntry::new(info.summary.name.clone(), page))
                })
                .collect();
        }
        Vec::new()
    }

    pub(crate) fn selected_plugin_settings(&self) -> Option<&str> {
        (self.section == Section::Plugin)
            .then_some(self.extensions.selected.as_deref())
            .flatten()
    }

    pub(crate) fn open_plugin_settings(&mut self, name: &str, cx: &mut Context<Self>) {
        self.deactivate(cx);
        self.section = Section::Plugin;
        self.extensions.selected = Some(name.into());
        if let Some(metadata) = &self.plugin_catalog.metadata {
            metadata.update(cx, |metadata, cx| metadata.refresh(cx));
        }
        cx.notify();
    }

    pub(super) fn plugin_full_width(&self, cx: &App) -> bool {
        self.selected_plugin_settings()
            .and_then(|name| {
                self.plugin_catalog
                    .metadata
                    .as_ref()?
                    .read(cx)
                    .entries
                    .get(name)?
                    .extension
                    .as_ref()?
                    .settings_page
                    .as_ref()
            })
            .is_some_and(|page| page.full_width)
    }

    pub(super) fn plugin_heading(&self, cx: &App) -> String {
        self.plugin_settings_entries(cx)
            .into_iter()
            .find(|entry| Some(entry.name.as_str()) == self.selected_plugin_settings())
            .map(|entry| entry.label)
            .unwrap_or_else(|| tr("plugins_settings_title").to_string())
    }

    pub(super) fn plugin_description(&self, cx: &App) -> String {
        let info = self.selected_plugin_settings().and_then(|name| {
            self.plugin_catalog
                .metadata
                .as_ref()?
                .read(cx)
                .entries
                .get(name)
        });
        info.and_then(|info| {
            info.extension
                .as_ref()
                .and_then(|extension| extension.description.as_ref())
                .map(|description| description.label(&rust_i18n::locale()).to_owned())
                .or_else(|| info.summary.description.clone())
        })
        .unwrap_or_else(|| tr("plugins_settings_description").to_string())
    }

    pub(super) fn plugin_switch(&self, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let summary = self
            .plugin_catalog
            .packages
            .iter()
            .find(|entry| entry.name == name)
            .cloned();
        let busy = self
            .provider_link
            .as_ref()
            .is_none_or(|live| !live.connected)
            || self.plugin_catalog.busy();
        let enabled = summary.as_ref().is_some_and(|plugin| plugin.enabled);
        Group::new("plugins_settings_title")
            .heading(false)
            .child(Row::new(
                "plugins_enabled",
                div()
                    .debug_selector(|| "plugin-settings-enabled".into())
                    .child(
                        Switch::new("plugin-settings-enabled")
                            .checked(enabled)
                            .disabled(busy || summary.is_none())
                            .accessibility_label(tr("plugins_enabled"))
                            .on_click(cx.listener(move |this, enabled, _, cx| {
                                if let Some(plugin) = &summary {
                                    this.toggle_plugin(plugin, *enabled, cx);
                                }
                            })),
                    ),
            ))
            .into_any_element()
    }

    pub(super) fn plugin_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(name) = self.selected_plugin_settings().map(str::to_owned) else {
            return div().into_any_element();
        };
        let info = self
            .plugin_catalog
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.read(cx).entries.get(&name).cloned());
        let Some(info) = info else {
            return div()
                .child(tr("plugins_view_unavailable"))
                .into_any_element();
        };
        let Some(page) = info
            .extension
            .as_ref()
            .and_then(|extension| extension.settings_page.as_ref())
        else {
            return div()
                .child(tr("plugins_view_unavailable"))
                .into_any_element();
        };
        if page.entry.is_none() {
            if info.settings.is_none() {
                return v_flex()
                    .gap_4()
                    .child(self.plugin_switch(&name, cx))
                    .child(
                        Group::new("computer_device").child(Row::new(
                            "computer_device",
                            div()
                                .debug_selector(|| "plugin-settings-node".into())
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    self.provider_link
                                        .as_ref()
                                        .map(|live| live.binding.label.clone())
                                        .unwrap_or_else(|| tr("composer_host_local")),
                                ),
                        )),
                    )
                    .into_any_element();
            }
            return self.schema_settings(info, window, cx);
        }
        if self.extensions.panel.is_none() {
            let Some(live) = &self.provider_link else {
                return div().into_any_element();
            };
            let binding = crate::conversation::live::Binding {
                client: live.binding.client.clone(),
                defaults: live.binding.client.clone(),
                runtime: live.binding.runtime.clone(),
                project: None,
                worktree: None,
                host: live.binding.label.clone(),
                project_name: "".into(),
                branch: "".into(),
            };
            let mut sources = self.usage_sources.clone();
            if !sources
                .iter()
                .any(|source| source.client.target() == binding.client.target())
            {
                sources.push(crate::plugins::usage::Source {
                    client: binding.client.clone(),
                    label: binding.host.to_string(),
                });
            }
            let panel = cx.new(|cx| {
                let panel = Panel::settings(binding, cx);
                panel.usage_sources.set(sources);
                panel
            });
            Panel::observe_notifications(&panel, window, cx);
            cx.subscribe(
                &panel,
                |_, _, event: &crate::plugins::navigation::SelectSettingsHost, cx| {
                    cx.emit(SelectHost(event.0))
                },
            )
            .detach();
            let name = info.summary.name.clone();
            cx.observe_in(&panel, window, move |_, panel, window, cx| {
                // An unopened page has no draft authority to retain. Its own
                // inventory may be newer than the workspace's navigation cache.
                if let Some(expected) = panel.read(cx).ready_settings(&name, cx) {
                    panel.update(cx, |panel, cx| panel.open(expected.clone(), window, cx));
                }
                cx.notify();
            })
            .detach();
            self.extensions.panel = Some(panel);
        }
        if let (Some(panel), Some(live)) = (&self.extensions.panel, &self.provider_link) {
            let mut sources = self.usage_sources.clone();
            if !sources
                .iter()
                .any(|source| source.client.target() == live.binding.client.target())
            {
                sources.push(crate::plugins::usage::Source {
                    client: live.binding.client.clone(),
                    label: live.binding.label.to_string(),
                });
            }
            panel.read(cx).usage_sources.set(sources);
        }
        if page.full_width {
            return div()
                .size_full()
                .min_h_0()
                .min_w_0()
                .children(self.extensions.panel.clone())
                .into_any_element();
        }
        v_flex()
            .gap_4()
            .when(page.enabled_control, |content| {
                content.child(self.plugin_switch(&name, cx))
            })
            .child(
                div()
                    .h(px(560.))
                    .w_full()
                    .children(self.extensions.panel.clone()),
            )
            .into_any_element()
    }

    fn schema_settings(
        &mut self,
        info: Info,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.extensions.form.is_none() {
            let Some(live) = &self.provider_link else {
                return div().into_any_element();
            };
            self.extensions.form = Some(super::plugins::configuration::page(
                live.binding.clone(),
                info.summary,
                window,
                cx,
            ));
        }
        div()
            .children(self.extensions.form.clone())
            .into_any_element()
    }
}

pub(crate) struct SelectHost(pub sailry_protocol::NodeId);
impl EventEmitter<SelectHost> for Workspace {}
