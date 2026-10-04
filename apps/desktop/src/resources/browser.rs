//! Browser pages resolve ordinary resource renderers against the captured session.
use super::{SideResource, launcher::Destination};
use crate::{browser::Browser, plugins::Panel, shell::Shell};
use gpui_kit::*;
use sailry_protocol::plugin::desktop::ResourceKind;

impl Shell {
    pub(crate) fn open_browser_resource(
        &mut self,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let retry = url.clone();
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.open_browser_resource(retry.clone(), window, cx)
        }) {
            return;
        }
        if self.live.is_none() {
            self.side_resource = Some(SideResource::Tool(Destination::Browser));
            self.layout.panel_open[0] = true;
            cx.notify();
            return;
        }
        let browser = self.browser_for(self.session_scope.active, window, cx);
        self.mount_browser_page(browser, url, window, cx);
    }

    pub(crate) fn reveal_browser(
        &mut self,
        browser: Entity<Browser>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = browser.clone();
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.reveal_browser(next.clone(), window, cx)
        }) {
            return;
        }
        self.mount_browser_page(browser, None, window, cx);
    }

    fn mount_browser_page(
        &mut self,
        browser: Entity<Browser>,
        mut url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.current_chat().cloned() else {
            return;
        };
        if let Some(SideResource::Plugin(panel)) = &self.side_resource
            && panel.read(cx).browser_view().as_ref() == Some(&browser)
            && panel.read(cx).resource_active()
        {
            if let Some(url) = url {
                browser.update(cx, |browser, cx| browser.open(&url, window, cx));
            }
            self.layout.panel_open[0] = true;
            cx.notify();
            return;
        }
        let panel = cx.new(|cx| Panel::browser(source, browser.clone(), cx));
        Self::observe_plugin_conversations(&panel, window, cx);
        cx.observe_in(&panel, window, move |shell, panel, window, cx| {
            if let Some((_, package)) = panel.read(cx).renderer_entry(ResourceKind::Browser, cx)
                && panel.read(cx).ready_for(&package, cx)
            {
                panel.update(cx, |panel, cx| panel.open(package, window, cx));
            }
            if panel.read(cx).resource_active()
                && let Some(url) = url.take()
            {
                browser.update(cx, |browser, cx| browser.open(&url, window, cx));
            }
            if matches!(&shell.side_resource, Some(SideResource::Plugin(current)) if *current == panel) {
                cx.notify();
            }
        })
        .detach();
        self.side_resource = Some(SideResource::Plugin(panel));
        self.layout.panel_open[0] = true;
        cx.notify();
    }
}
