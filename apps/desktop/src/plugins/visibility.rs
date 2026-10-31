//! Retained package controllers follow the existing navigation and dock owners.
use crate::{preview::Page, resources::SideResource, shell::Shell};
use gpui_kit::App;

fn set(resource: &SideResource, visible: bool, cx: &mut App) {
    if let SideResource::Plugin(panel) | SideResource::Launcher(Some(panel)) = resource {
        panel.read(cx).set_visible(visible);
    }
    if let SideResource::Child(panel) = resource {
        for tab in &panel.tabs {
            tab.view.update(cx, |view, cx| {
                view.set_visibility(visible && tab.id == panel.selected, cx)
            });
        }
    }
}

impl Shell {
    pub(crate) fn sync_plugin_visibility(&self, details: bool, cx: &mut App) {
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
        let workspace = self
            .page
            .workspace_visible(self.terminal_workspace_visible(cx));
        let visibility: Vec<_> = self
            .chats
            .controllers()
            .map(|chat| {
                let dock = self.splits.read(cx);
                let view = chat.read(cx);
                let target = view
                    .session()
                    .map_or(crate::panes::Target::Draft, |session| {
                        crate::panes::Target::Session(view.binding().client.target(), session)
                    });
                (chat.clone(), workspace && dock.displayed(target, cx))
            })
            .collect();
        for (chat, visible) in visibility {
            chat.update(cx, |view, cx| view.set_visibility(visible, cx));
        }
        let dock = self.splits.read(cx);
        for (target, panel) in &self.plugin_panes.panels {
            panel
                .read(cx)
                .set_visible(workspace && dock.displayed(*target, cx));
        }
    }
}
