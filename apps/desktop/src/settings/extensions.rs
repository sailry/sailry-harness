//! Manifest-contributed settings share navigation and captured Node ownership.
pub(crate) mod modal;
use super::{
    Section, Workspace,
    group::{Group, Row},
};
use crate::{plugins::Panel, tr};
use gpui_kit::{component::*, *};
use sailry_protocol::plugin::{Info, Reference, desktop};
use std::collections::BTreeMap;

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
    panels: BTreeMap<String, (Reference, Entity<Panel>)>,
    panel: Option<Entity<Panel>>,
    form: Option<Entity<super::plugins::configuration::Editor>>,
    resources: Option<crate::plugins::preload::Resources>,
    warmed: bool,
}

impl State {
    pub(super) fn reset(selected: Option<String>) -> Self {
        Self {
            selected,
            ..Default::default()
        }
    }

    pub(super) fn park(&mut self, cx: &App) {
        if let Some(panel) = self.panel.take() {
            panel.read(cx).activity_sources.set_visible(false);
        }
        self.form = None;
    }

    pub(super) fn release(&mut self, cx: &mut App) {
        self.park(cx);
        for (_, (_, panel)) in std::mem::take(&mut self.panels) {
            panel.update(cx, |panel, cx| panel.close(cx));
        }
    }

    pub(super) fn retain(&mut self, summaries: &[sailry_protocol::plugin::Summary], cx: &mut App) {
        if let Some(resources) = &mut self.resources {
            resources.retain(summaries);
        }
        let active = self.panel.clone();
        let mut released = false;
        self.panels.retain(|name, (package, panel)| {
            if summaries
                .iter()
                .any(|summary| summary.name == *name && summary.digest == package.digest)
            {
                return true;
            }
            released |= active.as_ref() == Some(panel);
            panel.update(cx, |panel, cx| panel.close(cx));
            false
        });
        if released {
            self.panel = None;
        }
    }

    pub(super) fn clear_resources(&mut self) {
        self.resources = None;
        self.warmed = false;
    }
}

impl Workspace {
    pub(crate) fn preload_settings(&mut self, name: &str, cx: &App) {
        let _ = self.prepare_settings(name, cx);
    }

    fn prepare_settings(
        &mut self,
        name: &str,
        cx: &App,
    ) -> Option<crate::plugins::preload::Prepared> {
        let live = self.provider_link.as_ref().filter(|live| live.connected)?;
        let info = self
            .plugin_catalog
            .metadata
            .as_ref()?
            .read(cx)
            .entries
            .get(name)?;
        info.extension
            .as_ref()?
            .settings_page
            .as_ref()?
            .entry
            .as_ref()?;
        let package = info.summary.reference();
        let resources = self.extensions.resources.get_or_insert_with(|| {
            crate::plugins::preload::Resources::new(
                live.binding.client.clone(),
                live.binding.runtime.handle().clone(),
            )
        });
        Some(resources.prepare(package))
    }

    pub(super) fn warm_settings(&mut self, cx: &App) {
        if self.section.is_extension()
            || self.extensions.warmed
            || !self
                .provider_link
                .as_ref()
                .is_some_and(|live| live.connected)
        {
            return;
        }
        let Some(metadata) = self
            .plugin_catalog
            .metadata
            .as_ref()
            .map(|metadata| metadata.read(cx))
        else {
            return;
        };
        if !metadata.settled() || metadata.entries.is_empty() {
            return;
        }
        // Passive warming must not evict a resource already requested by navigation.
        let remaining = self.extensions.resources.as_ref().map_or(
            crate::plugins::preload::CAPACITY,
            crate::plugins::preload::Resources::remaining,
        );
        let names: Vec<_> = metadata
            .entries
            .values()
            .filter(|info| {
                info.extension
                    .as_ref()
                    .and_then(|extension| extension.settings_page.as_ref())
                    .is_some_and(|page| page.entry.is_some())
            })
            .take(remaining)
            .map(|info| info.summary.name.clone())
            .collect();
        self.extensions.warmed = true;
        for name in names {
            self.preload_settings(&name, cx);
        }
    }
    pub(crate) fn configure_plugin(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.provider_link else {
            return;
        };
        if !live.connected {
            return;
        }
        let Some(info) = self
            .plugin_catalog
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.read(cx).entries.get(name))
            .cloned()
        else {
            return;
        };
        if modal::has_panel(&info) {
            let event = modal::OpenPanel {
                node: live.binding.client.target(),
                name: name.into(),
            };
            self.open_plugin_settings(name, cx);
            cx.emit(event);
        } else {
            modal::open(live.binding.clone(), info, window, cx);
        }
    }

    pub(crate) fn configure_bound(
        binding: crate::conversation::live::Binding,
        info: Info,
        window: &mut Window,
        cx: &mut App,
    ) {
        modal::open(
            super::providers::Binding {
                client: binding.client,
                runtime: binding.runtime,
                label: binding.host,
            },
            info,
            window,
            cx,
        );
    }

    #[cfg(test)]
    pub(crate) fn plugin_settings_panel(&self) -> Option<Entity<Panel>> {
        self.extensions.panel.clone()
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
                .filter(|info| modal::configurable(info))
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
        if !self.extensions.panels.contains_key(name) {
            self.preload_settings(name, cx);
        }
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
        if self.extensions.panel.is_none()
            && let Some((_, panel)) = self.extensions.panels.get(&name)
        {
            panel.update(cx, |panel, cx| {
                panel.enter(info.summary.reference(), window, cx)
            });
            self.extensions.panel = Some(panel.clone());
        }
        if self.extensions.panel.is_none() {
            let prepared = self.prepare_settings(&name, cx);
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
                let panel = Panel::settings(binding, !page.full_width, cx)
                    .with_target(info.summary.name.clone())
                    .with_prepared(prepared);
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
            self.extensions.panels.insert(
                info.summary.name.clone(),
                (info.summary.reference(), panel.clone()),
            );
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
            .child(div().w_full().children(self.extensions.panel.clone()))
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
