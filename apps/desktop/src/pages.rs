use gpui_kit::component::*;
use gpui_kit::*;

use crate::{preview::Page, shell::Shell, tr};

impl Shell {
    pub fn page_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if matches!(self.page, Page::Settings | Page::Plugins)
            && let Some(live) = &self.live
        {
            let node = self.settings_target.unwrap_or(live.selected);
            let Some(transport) = live.transport_for(node) else {
                self.settings
                    .update(cx, |settings, cx| settings.deactivate(cx));
                return div().size_full().into_any_element();
            };
            let runtime = cx.global::<crate::backend::Services>().runtime.clone();
            let label = live.name(node);
            let sources = self.usage_sources();
            self.settings.update(cx, |settings, cx| {
                settings.bind_providers(transport, runtime, label, cx);
                settings.usage_sources = sources;
            });
        }
        if self.page == Page::Plugin {
            if self.needs_project() {
                return self.project_empty(cx);
            }
            return self
                .extensions
                .as_ref()
                .and_then(|state| state.panel.clone())
                .map(|panel| panel.into_any_element())
                .unwrap_or_else(|| div().size_full().into_any_element());
        }
        if self.page == Page::Git && self.live.is_none() {
            return if self.needs_project() {
                self.project_empty(cx)
            } else {
                self.git_diff(false, cx)
            };
        }
        if self.page == Page::Files && self.live.is_none() {
            return if self.needs_project() {
                self.project_empty(cx)
            } else {
                self.file_preview(false, cx)
            };
        }
        if self.live.is_some() && matches!(self.page, Page::Host | Page::Project) {
            self.bind_ports(window, cx);
            if self.page == Page::Host {
                return self.live_host_overview(cx);
            }
            return self.live_overview(window, cx);
        }
        if self.live.is_some() && self.page == Page::Terminal {
            return self.live_terminal_page(window, cx);
        }
        if self.live.is_some() && self.page == Page::Conversation {
            return self.split_content(window, cx);
        }
        let content = match self.page {
            Page::Conversation => self.conversation(cx),
            Page::Settings | Page::Plugins => self.settings.clone().into_any_element(),
            Page::Host => self.host_overview(cx),
            Page::Project => self.project_overview(cx),
            Page::Terminal => self.terminal_overview(cx),
            _ => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p_6()
                .gap_3()
                .child(Icon::new(self.page.icon()).large())
                .child(tr("unavailable"))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("unavailable_description")),
                )
                .into_any_element(),
        };
        if self.live.is_some()
            && !matches!(
                self.page,
                Page::Settings | Page::Plugins | Page::Files | Page::Git
            )
        {
            v_flex()
                .size_full()
                .child(
                    div()
                        .px_6()
                        .py_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("live_preview_scope")),
                )
                .child(div().flex_1().min_h_0().child(content))
                .into_any_element()
        } else {
            content
        }
    }
}
