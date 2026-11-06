//! Workspace header composition; drag and sizing live in the shared Header.
use super::*;
use gpui_kit::component::separator::Separator;

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
                Page::Settings | Page::Market => self
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

    fn header_actions(
        &self,
        pane_actions: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let filters = self.header_has_filters(cx);
        let plugin_header = if self.page == Page::Plugin {
            self.extensions
                .as_ref()
                .and_then(|state| state.panel.as_ref())
                .and_then(|panel| panel.read(cx).header())
                .filter(|header| !header.read(cx).is_empty())
        } else {
            None
        };
        let settings = if self.page == Page::Plugin {
            self.extension_settings(cx)
        } else {
            None
        };
        let more =
            self.page == Page::Project || (self.page == Page::Conversation && self.live.is_none());
        if pane_actions.is_none() && plugin_header.is_none() && settings.is_none() && !more {
            return None;
        }
        Some(
            h_flex()
                .debug_selector(|| "header-actions".into())
                .flex_shrink_0()
                .when(filters, |row| row.flex_1().min_w_0())
                .gap_2()
                .children(pane_actions)
                .children(plugin_header)
                .when_some(settings, |row, entry| {
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
                .when(more, |row| {
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
                }),
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
        let width = if cfg!(target_os = "windows") {
            let controls = window.window_controls();
            let count = 1 + u8::from(controls.minimize) + u8::from(controls.maximize);
            width - gpui_kit::component::TITLE_BAR_HEIGHT * f32::from(count)
        } else {
            width
        };
        let filters = self.header_has_filters(cx);
        let split = matches!(self.page, Page::Conversation | Page::Terminal)
            && self.splits.read(cx).is_split(cx);
        let (pane_title, pane_actions) =
            if matches!(self.page, Page::Conversation | Page::Terminal) && self.live.is_some() {
                self.splits
                    .read(cx)
                    .single_header(cx)
                    .map_or((None, None), |(title, actions)| (Some(title), actions))
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
        let activity = self.activity_strip(width, cx);
        let actions = self.header_actions(pane_actions, cx);
        let details = self
            .has_resource_panel(cx)
            .then(|| self.details_toggle(visible, cx));
        let has_actions = activity.is_some() || actions.is_some() || details.is_some();
        crate::header::Header::new("shell-module-header", cx)
            .bordered(false)
            .draggable(!cfg!(target_os = "windows"))
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
                    .children(activity)
                    .children(actions)
                    .children(details)
                    .when(cfg!(target_os = "windows") && has_actions, |row| {
                        row.child(Separator::vertical().relative().w_px().h_4())
                    }),
            )
    }
}
