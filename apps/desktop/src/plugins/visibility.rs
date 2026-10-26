//! Retained package controllers follow the existing navigation and dock owners.
use crate::{preview::Page, resources::SideResource, shell::Shell};
use gpui_kit::App;

fn set(resource: &SideResource, visible: bool, cx: &App) {
    if let SideResource::Plugin(panel) | SideResource::Launcher(Some(panel)) = resource {
        panel.read(cx).set_visible(visible);
    }
    if let SideResource::Child(panel) = resource {
        for tab in &panel.tabs {
            tab.view
                .read(cx)
                .set_plugin_visibility(visible && tab.id == panel.selected, cx);
        }
    }
}

impl Shell {
    pub(crate) fn sync_plugin_visibility(&self, details: bool, cx: &App) {
        for resource in self.retained_panels() {
            set(resource, false, cx);
        }
        if let Some(resource) = &self.side_resource {
            set(resource, details && self.page == Page::Conversation, cx);
        }
        if let Some(navigation) = &self.extensions {
            for panel in navigation
                .panels
                .iter()
                .map(|(_, panel)| panel)
                .chain(navigation.panel.iter())
            {
                panel.read(cx).set_visible(
                    self.page == Page::Plugin && navigation.panel.as_ref() == Some(panel),
                );
            }
        }
        let dock = self.splits.read(cx);
        for chat in self.chats.controllers() {
            let view = chat.read(cx);
            let target = view
                .session()
                .map_or(crate::panes::Target::Draft, |session| {
                    crate::panes::Target::Session(view.binding().client.target(), session)
                });
            view.set_plugin_visibility(
                self.page == Page::Conversation && dock.displayed(target, cx),
                cx,
            );
        }
        for (target, panel) in &self.plugin_panes.panels {
            panel.read(cx).set_visible(
                matches!(self.page, Page::Conversation | Page::Terminal)
                    && dock.displayed(*target, cx),
            );
        }
    }
}
