use super::*;
use crate::resources::SideResource;

pub(crate) struct Panel {
    pub(crate) resource: Option<SideResource>,
    open: bool,
    width: f32,
    resized: bool,
}

impl Shell {
    pub(crate) fn park_session_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.session_scope.mounted.take() else {
            return;
        };
        if let Some(browser) = self
            .side_resource
            .as_ref()
            .and_then(|panel| panel.browser(cx))
        {
            browser.update(cx, |browser, cx| browser.visibility(false, window, cx));
        }
        self.session_scope.panels.insert(
            key,
            Panel {
                resource: self.side_resource.take(),
                open: self.layout.panel_open[0],
                width: self.layout.panel_width[0],
                resized: self.layout.conversation_panel_resized,
            },
        );
        self.layout.panel_open[0] = false;
    }

    pub(crate) fn sync_session_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = (self.page == Page::Conversation).then_some(self.session_scope.active);
        if target == self.session_scope.mounted {
            return;
        }
        self.park_session_panel(window, cx);
        if let Some(key) = target {
            if let Some(panel) = self.session_scope.panels.remove(&key) {
                self.side_resource = panel.resource;
                self.layout.panel_open[0] = panel.open && self.side_resource.is_some();
                self.layout.panel_width[0] = panel.width;
                self.layout.conversation_panel_resized = panel.resized;
            } else {
                self.layout.conversation_panel_resized = false;
            }
            self.session_scope.mounted = Some(key);
        }
    }

    pub(crate) fn retained_panels(&self) -> impl Iterator<Item = &SideResource> {
        self.session_scope
            .panels
            .values()
            .filter_map(|panel| panel.resource.as_ref())
    }
}
