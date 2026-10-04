//! Workspace header composition; drag and sizing live in the shared Header.
use super::*;

impl Shell {
    fn header_title(
        &self,
        pane_title: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        h_flex()
            .debug_selector(|| "resource-title".into())
            .flex_1()
            .min_w_0()
            .when(self.page != Page::Conversation, |title| title.h_full())
            .text_sm()
            .child(match self.page {
                Page::Conversation | Page::Terminal if pane_title.is_some() => div()
                    .debug_selector(|| "single-pane-title".into())
                    .truncate()
                    .child(pane_title.unwrap())
                    .into_any_element(),
                Page::Settings | Page::Plugins => self
                    .settings
                    .update(cx, |settings, cx| settings.header(window, cx))
                    .unwrap_or_else(|| div().truncate().child(self.title()).into_any_element()),
                Page::Plugin => {
                    let header = self
                        .extensions
                        .as_ref()
                        .and_then(|state| state.panel.as_ref())
                        .and_then(|panel| panel.read(cx).header());
                    let title = header
                        .and_then(|header| header.read(cx).title(cx))
                        .unwrap_or_else(|| {
                            div()
                                .truncate()
                                .child(
                                    self.extensions
                                        .as_ref()
                                        .map(|state| state.label.clone())
                                        .unwrap_or_else(|| tr("settings_plugins")),
                                )
                                .into_any_element()
                        });
                    h_flex()
                        .h_full()
                        .min_w_0()
                        .flex_1()
                        .gap_3()
                        .child(title)
                        .into_any_element()
                }
                Page::Conversation if self.live.is_some() => div().into_any_element(),
                Page::Conversation => div().truncate().child(self.title()).into_any_element(),
                Page::Files if !self.needs_project() && !self.files.tabs.open.is_empty() => {
                    self.document_tabs(Page::Files, false, cx)
                }
                Page::Git if !self.needs_project() && !self.git.tabs.open.is_empty() => {
                    self.document_tabs(Page::Git, false, cx)
                }
                _ => div().truncate().child(self.title()).into_any_element(),
            })
    }

    fn header_actions(&self, pane_actions: Option<AnyElement>, cx: &mut Context<Self>) -> Div {
        let filters = self.header_has_filters(cx);
        h_flex()
            .debug_selector(|| "header-actions".into())
            .flex_shrink_0()
            .when(filters, |row| row.flex_1().min_w_0())
            .gap_2()
            .children(pane_actions)
            .when(self.page == Page::Plugin, |row| {
                row.children(
                    self.extensions
                        .as_ref()
                        .and_then(|state| state.panel.as_ref())
                        .and_then(|panel| panel.read(cx).header()),
                )
                .when_some(self.extension_settings(cx), |row, entry| {
                    row.child(
                        Button::new("plugin-settings")
                            .debug_selector(|| "plugin-settings".into())
                            .ghost()
                            .small()
                            .icon(IconName::Settings)
                            .tooltip(tr("plugins_settings_title"))
                            .accessibility_label(tr("plugins_settings_title"))
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.configure_extension(entry.clone(), window, cx)
                            })),
                    )
                })
            })
            .when(
                self.page == Page::Project
                    || (self.page == Page::Conversation && self.live.is_none()),
                |row| {
                    row.child(
                        Button::new("more")
                            .debug_selector(|| "header-more".into())
                            .ghost()
                            .small()
                            .icon(IconName::Ellipsis)
                            .tooltip(tr("more"))
                            .accessibility_label(tr("more"))
                            .map(|button| {
                                if self.page == Page::Project {
                                    self.project_menu(button, cx).into_any_element()
                                } else {
                                    self.conversation_menu(button, cx).into_any_element()
                                }
                            }),
                    )
                },
            )
    }

    fn details_toggle(&self, visible: bool, cx: &mut Context<Self>) -> Button {
        let label = self
            .plugin_workspace(cx)
            .and_then(|workspace| workspace.read(cx).details_label())
            .unwrap_or_else(|| {
                tr(match self.page {
                    Page::Host => "metrics_title",
                    _ => "details",
                })
            });
        Button::new("details")
            .debug_selector(|| "toggle-details".into())
            .ghost()
            .small()
            .icon(IconName::PanelRight)
            .selected(visible)
            .tooltip(label.clone())
            .accessibility_label(label)
            .on_click(
                cx.listener(|this, _, window, cx| this.toggle_details(&ToggleDetails, window, cx)),
            )
    }
}

impl Shell {
    fn header_has_filters(&self, cx: &App) -> bool {
        self.page == Page::Plugin
            && self
                .extensions
                .as_ref()
                .and_then(|state| state.panel.as_ref())
                .and_then(|panel| panel.read(cx).header())
                .is_some_and(|header| header.read(cx).has_filters())
    }

    pub(crate) fn sidebar_toggle(&self, cx: &mut Context<Self>) -> Button {
        Button::new("header-sidebar-toggle")
            .debug_selector(|| "header-sidebar-toggle".into())
            .ghost()
            .small()
            .icon(if self.layout.sidebar_open {
                IconName::PanelLeftClose
            } else {
                IconName::PanelLeftOpen
            })
            .accessibility_label(tr("sidebar_toggle"))
            .on_click(
                cx.listener(|shell, _, window, cx| {
                    shell.toggle_sidebar(&ToggleSidebar, window, cx)
                }),
            )
    }
}

impl Shell {
    pub(super) fn module_header(
        &self,
        navigation: bool,
        visible: bool,
        width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let filters = self.header_has_filters(cx);
        let split = matches!(self.page, Page::Conversation | Page::Terminal)
            && self.splits.read(cx).is_split(cx);
        let (pane_title, pane_actions) =
            if matches!(self.page, Page::Conversation | Page::Terminal) && self.live.is_some() {
                self.splits.read(cx).single_header(cx).unzip()
            } else {
                (None, None)
            };
        let title = if split {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(self.page.label())
                .into_any_element()
        } else {
            self.header_title(pane_title, window, cx).into_any_element()
        };
        crate::header::Header::new("shell-module-header", cx)
            .bordered(false)
            .child(
                h_flex()
                    .min_w_0()
                    .h_full()
                    .flex_1()
                    .when(filters, |row| row.flex_none().max_w(rems(12.)))
                    .gap_2()
                    .when(navigation, |row| row.child(self.sidebar_toggle(cx)))
                    .child(title),
            )
            .child(
                h_flex()
                    .h_full()
                    .min_w_0()
                    .when(filters, |row| row.flex_1())
                    .justify_end()
                    .gap_2()
                    .children(self.activity_strip(width, cx))
                    .child(self.header_actions(pane_actions, cx))
                    .when(self.has_resource_panel(cx), |row| {
                        row.child(self.details_toggle(visible, cx))
                    }),
            )
    }
}
