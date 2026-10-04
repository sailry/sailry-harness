//! Native viewport only. Package UI owns tabs, address editing, and controls.
use super::*;

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.tabs.is_empty() {
            return div().into_any_element();
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if self.reload {
            self.reload = false;
            let url = self.tabs[self.selected].url.clone();
            if !url.is_empty() && !self.preview {
                self.load(url, window, cx);
            }
        }
        self.visibility(self.mounted, window, cx);
        let mut body = div().into_any_element();
        let tab = &self.tabs[self.selected];
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(page) = &tab.page {
            body = page.clone().into_any_element();
        }
        #[cfg(target_os = "macos")]
        if let (Some(surface), Some(page)) = (&tab.surface, &tab.page) {
            body = surface.element(page, cx);
        }
        let panel = div().id("browser-surface").key_context("SailryBrowser");
        #[cfg(target_os = "macos")]
        let panel =
            panel.on_action(cx.listener(|this, _: &inspector::Inspect, _, cx| this.inspect(cx)));
        panel
            .debug_selector(|| "browser-surface".into())
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(body)
            .into_any_element()
    }
}
