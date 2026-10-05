use super::SideResource;
use crate::{preview::Page, shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    kbd::Kbd,
    *,
};
use gpui_kit::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Destination {
    Review,
    Terminal,
    Browser,
    Files,
}

impl Destination {
    fn key(self) -> &'static str {
        match self {
            Self::Review => "resource_review",
            Self::Terminal => "terminal",
            Self::Browser => "resource_browser",
            Self::Files => "files",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Review => Page::Git.icon(),
            Self::Terminal => Page::Terminal.icon(),
            Self::Browser => IconName::Globe,
            Self::Files => IconName::Folder,
        }
    }

    fn resource(self) -> sailry_protocol::plugin::desktop::ResourceKind {
        use sailry_protocol::plugin::desktop::ResourceKind;
        match self {
            Self::Review => ResourceKind::Git,
            Self::Terminal => ResourceKind::Terminal,
            Self::Browser => ResourceKind::Browser,
            Self::Files => ResourceKind::Documents,
        }
    }
}

impl From<sailry_protocol::plugin::desktop::ResourceKind> for Destination {
    fn from(resource: sailry_protocol::plugin::desktop::ResourceKind) -> Self {
        use sailry_protocol::plugin::desktop::ResourceKind;
        match resource {
            ResourceKind::Git => Self::Review,
            ResourceKind::Terminal => Self::Terminal,
            ResourceKind::Browser => Self::Browser,
            ResourceKind::Documents => Self::Files,
        }
    }
}

impl Shell {
    pub(crate) fn open_destination(
        &mut self,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if destination == Destination::Review {
            if self.live.is_some() {
                if let Some(binding) = self.current_chat().map(|chat| chat.read(cx).binding())
                    && let Some(tree) = binding.worktree
                {
                    self.open_git_resource((binding.client.target(), tree), None, window, cx);
                }
            } else {
                self.open_resource_panel(Page::Git, window, cx);
            }
            return;
        }
        if destination == Destination::Files {
            if self.live.is_some() {
                if let Some(binding) = self.current_chat().map(|chat| chat.read(cx).binding())
                    && let Some(worktree) = binding.worktree
                {
                    self.open_documents((binding.client.target(), worktree), None, window, cx);
                }
            } else {
                self.open_resource_panel(Page::Files, window, cx);
            }
            return;
        }
        if destination == Destination::Browser {
            self.open_browser_resource(None, window, cx);
            return;
        }
        if destination == Destination::Terminal {
            if self.live.is_some() {
                if self.guard_file_navigation(window, cx, |shell, window, cx| {
                    shell.open_destination(Destination::Terminal, window, cx)
                }) {
                    return;
                }
                self.terminal_action(None, window, cx);
            } else {
                self.side_resource = Some(SideResource::Tool(destination));
                self.layout.panel_open[0] = true;
                cx.notify();
            }
        }
    }

    pub(super) fn resource_launcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let plugins = match &self.side_resource {
            Some(SideResource::Launcher(Some(panel))) => Some(panel.clone()),
            _ => None,
        };
        let entries = plugins
            .as_ref()
            .map(|panel| panel.read(cx).launcher_entries(cx))
            .unwrap_or_default();
        let mut controls = Vec::new();
        if let Some((label, package)) = plugins.as_ref().and_then(|panel| {
            panel
                .read(cx)
                .renderer_entry(sailry_protocol::plugin::desktop::ResourceKind::Terminal, cx)
        }) {
            controls.push((package.name, Destination::Terminal, label));
        } else if self.live.is_none() {
            controls.push((
                "terminal".into(),
                Destination::Terminal,
                tr("terminal").to_string(),
            ));
        }
        if let Some((label, package)) = plugins.as_ref().and_then(|panel| {
            panel
                .read(cx)
                .renderer_entry(sailry_protocol::plugin::desktop::ResourceKind::Browser, cx)
        }) {
            controls.push((package.name, Destination::Browser, label));
        } else if self.live.is_none() {
            controls.push((
                "browser".into(),
                Destination::Browser,
                tr("resource_browser").to_string(),
            ));
        }
        if let Some((label, package)) = plugins.as_ref().and_then(|panel| {
            panel.read(cx).renderer_entry(
                sailry_protocol::plugin::desktop::ResourceKind::Documents,
                cx,
            )
        }) {
            controls.push((package.name, Destination::Files, label));
        } else if self.live.is_none() {
            controls.push(("files".into(), Destination::Files, tr("files").to_string()));
        }
        if let Some((label, package)) = plugins.as_ref().and_then(|panel| {
            panel
                .read(cx)
                .renderer_entry(sailry_protocol::plugin::desktop::ResourceKind::Git, cx)
        }) {
            controls.push((package.name, Destination::Review, label));
        } else if self.live.is_none() {
            controls.push(("git".into(), Destination::Review, tr("git").to_string()));
        }
        controls.sort_by_key(|(_, destination, _)| match destination {
            Destination::Browser => 0,
            Destination::Terminal => 1,
            Destination::Files => 2,
            Destination::Review => 3,
        });
        v_flex()
            .debug_selector(|| "resource-launcher".into())
            .size_full()
            .child(
                crate::header::Header::new("resource-launcher-header", cx)
                    .bordered(false)
                    .child(tr("details")),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .items_center()
                    .justify_center()
                    .px_6()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(440.))
                            .gap_2()
                            .children(controls.into_iter().map(|(_, destination, label)| {
                                Button::new(destination.key())
                                    .debug_selector(move || format!("launch-{}", destination.key()))
                                    .secondary()
                                    .bg(cx.theme().secondary)
                                    .border_0()
                                    .w_full()
                                    .h_10()
                                    .px_3()
                                    .accessibility_label(label.clone())
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .gap_3()
                                            .child(destination.icon())
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_sm()
                                                    .text_left()
                                                    .child(label.clone()),
                                            )
                                            .children(
                                                self.resource_shortcuts(cx)
                                                    .into_iter()
                                                    .find_map(|(resource, key)| {
                                                        (resource == destination.resource())
                                                            .then_some(key)
                                                    })
                                                    .into_iter()
                                                    .flat_map(|key| {
                                                        key.split_whitespace()
                                                            .filter_map(|part| {
                                                                Keystroke::parse(part).ok()
                                                            })
                                                            .map(Kbd::new)
                                                            .collect::<Vec<_>>()
                                                    }),
                                            ),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_destination(destination, window, cx)
                                    }))
                            }))
                            .children(entries.into_iter().enumerate().map(
                                |(index, (name, package))| {
                                    let panel = plugins.clone().unwrap();
                                    Button::new(("launch-extension", index))
                                        .debug_selector(move || format!("launch-extension-{index}"))
                                        .secondary()
                                        .bg(cx.theme().secondary)
                                        .border_0()
                                        .w_full()
                                        .h_10()
                                        .px_3()
                                        .accessibility_label(name.clone())
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .gap_3()
                                                .child(IconName::GalleryVerticalEnd)
                                                .child(div().text_sm().child(name)),
                                        )
                                        .on_click(cx.listener(move |shell, _, window, cx| {
                                            panel.update(cx, |panel, cx| {
                                                panel.open(package.clone(), window, cx)
                                            });
                                            shell.side_resource =
                                                Some(SideResource::Plugin(panel.clone()));
                                            cx.notify();
                                        }))
                                },
                            )),
                    ),
            )
            .into_any_element()
    }

    #[cfg(test)]
    pub(crate) fn open_plugin_panel(
        &mut self,
        source: Entity<crate::conversation::live::View>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != Page::Conversation || self.current_chat() != Some(&source) {
            return;
        }
        let original = source.clone();
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.open_plugin_panel(original.clone(), window, cx)
        }) {
            return;
        }
        let panel = cx.new(|cx| crate::plugins::Panel::new(source, cx));
        Self::observe_plugin_conversations(&panel, window, cx);
        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
        self.side_resource = Some(SideResource::Plugin(panel));
        self.layout.panel_open[0] = true;
        cx.notify();
    }

    pub(super) fn resource_tool_preview(
        &self,
        destination: Destination,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .debug_selector(|| "resource-tool-preview".into())
            .size_full()
            .child(
                crate::header::Header::new("resource-tool-header", cx)
                    .bordered(false)
                    .child(destination.icon())
                    .child(div().flex_1().child(tr(destination.key()))),
            )
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .gap_3()
                    .child(destination.icon())
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(match destination {
                                Destination::Terminal => "resource_terminal_preview",
                                _ => "resource_browser_preview",
                            })),
                    ),
            )
            .into_any_element()
    }
}

impl Shell {
    pub(crate) fn open_artifact(
        &mut self,
        source: Entity<crate::conversation::live::View>,
        worktree: sailry_protocol::WorktreeId,
        file: sailry_protocol::tool::File,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let original = source.clone();
        let retry = file.clone();
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.open_artifact(original.clone(), worktree, retry.clone(), window, cx)
        }) {
            return;
        }
        let binding = source.read(cx).binding();
        let panel =
            cx.new(|cx| crate::content::files::Panel::new(source, binding, worktree, file, cx));
        self.side_resource = Some(SideResource::Artifact(panel));
        self.layout.panel_open[0] = true;
        cx.notify();
    }
}
