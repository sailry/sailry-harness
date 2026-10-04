mod browser;
pub(crate) mod checkpoints;
mod close;
mod document_actions;
mod document_footer;
mod documents;
pub(crate) mod file_open;
pub(crate) mod file_source;
mod git;
pub(crate) mod launcher;
mod preview_files;
mod preview_git;
pub(crate) mod rows;
pub(crate) mod shortcuts;
pub(crate) mod split;
mod tabs;
mod worktrees;

pub(crate) use document_actions::{
    CopySelection, CutSelection, ExtendFilesDown, ExtendFilesUp, PasteSelection, RenameSelection,
    SaveFile, SelectFiles,
};
pub(crate) use preview_files::PreviewFiles;
pub(crate) use preview_git::PreviewGit;

use crate::{preview::Page, shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    *,
};
use gpui_kit::*;

pub(crate) fn init(cx: &mut App) {
    document_actions::init(cx);
}

pub(crate) const MIN_WIDTH: f32 = split::MIN_WIDTH * 2.;

pub(crate) enum SideResource {
    Launcher(Option<Entity<crate::plugins::Panel>>),
    Tool(launcher::Destination),
    Artifact(Entity<crate::content::files::Panel>),
    PreviewFiles(Box<PreviewFiles>),
    PreviewGit(Box<PreviewGit>),
    Link(SharedString),
    Subagent(crate::conversation::subagent::Panel),
    Child(crate::conversation::live::subagents::Panel),
    Plugin(Entity<crate::plugins::Panel>),
}

impl SideResource {
    pub(crate) fn browser(&self, cx: &App) -> Option<Entity<crate::browser::Browser>> {
        match self {
            Self::Plugin(panel) => panel.read(cx).browser_view(),
            _ => None,
        }
    }

    pub fn page(&self) -> Page {
        match self {
            Self::PreviewFiles(_) => Page::Files,
            Self::PreviewGit(_) => Page::Git,
            Self::Link(_)
            | Self::Launcher(_)
            | Self::Tool(_)
            | Self::Artifact(_)
            | Self::Subagent(_)
            | Self::Child(_)
            | Self::Plugin(_) => Page::Conversation,
        }
    }
}

impl Shell {
    pub(crate) fn has_resource_panel(&self, cx: &App) -> bool {
        if self.page == Page::Plugin {
            return self
                .plugin_workspace(cx)
                .is_some_and(|workspace| workspace.read(cx).has_details());
        }
        if self.page == Page::Conversation && self.live.is_some() && self.current_chat().is_none() {
            return false;
        }
        if !self.page.has_panel() || self.needs_project() {
            return false;
        }
        true
    }

    pub(crate) fn needs_project(&self) -> bool {
        (matches!(self.page, Page::Files | Page::Git) || self.plugin_worktree())
            && self.live.as_ref().map_or_else(
                || self.workspace.selected_owner(self.host).is_none(),
                |live| {
                    if self.plugin_worktree() {
                        live.selected_worktree().is_none()
                    } else {
                        live.selected_project().is_none()
                    }
                },
            )
    }

    pub(crate) fn project_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        crate::empty_state::panel(self.page.icon(), "resource_project_empty", cx)
            .gap_4()
            .child(
                Button::new("empty-create-project")
                    .debug_selector(|| "empty-create-project".into())
                    .primary()
                    .icon(IconName::Plus)
                    .label(tr("project_new"))
                    .on_click(cx.listener(|shell, _, window, cx| {
                        shell.project_editor(shell.host, None, window, cx);
                    })),
            )
            .into_any_element()
    }

    pub(crate) fn sync_conversation_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stale = match &self.side_resource {
            Some(SideResource::Plugin(panel) | SideResource::Launcher(Some(panel))) => {
                self.page != Page::Conversation
                    || self.current_chat() != panel.read(cx).source.as_ref()
                    || !panel.read(cx).matches_source(cx)
            }
            Some(SideResource::Artifact(panel)) => {
                self.page != Page::Conversation
                    || self.current_chat() != Some(&panel.read(cx).source)
            }
            Some(SideResource::Child(panel)) => {
                self.page != Page::Conversation
                    || self.current_chat() != Some(&panel.source)
                    || panel.source.read(cx).child(panel.selected).is_none()
            }
            _ => false,
        };
        if stale {
            let promoted = self
                .side_resource
                .as_ref()
                .and_then(|panel| panel.browser(cx))
                .filter(|browser| {
                    self.page == Page::Conversation
                        && self
                            .browsers
                            .owns_session(self.session_scope.active, browser)
                });
            self.close_resource_panel(cx);
            if let Some(browser) = promoted {
                self.reveal_browser(browser, window, cx);
            }
        }
    }

    pub fn details(
        &self,
        width: Pixels,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.page {
            Page::Files if self.live.is_none() => self.file_explorer(false, None, cx),
            Page::Git if self.live.is_none() => self.git_changes(false, None, cx),
            Page::Plugin => div().into_any_element(),
            Page::Host => self
                .host_metrics
                .as_ref()
                .map(|panel| panel.clone().into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            _ => self.conversation_resource_panel(width, cx),
        }
    }

    fn conversation_resource_panel(&self, width: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let Some(panel) = &self.side_resource else {
            return div().into_any_element();
        };
        let page = panel.page();
        let content = match panel {
            SideResource::Launcher(_) => self.resource_launcher(cx),
            SideResource::Tool(destination) => self.resource_tool_preview(*destination, cx),
            SideResource::Artifact(panel) => panel.clone().into_any_element(),
            SideResource::Subagent(panel) => self.subagent_panel(panel, cx),
            SideResource::Child(panel) => self.live_child_panel(panel, cx),
            SideResource::Plugin(panel)
                if panel.read(cx).documents.is_some() || panel.read(cx).resource.is_some() =>
            {
                panel.update(cx, |panel, cx| panel.embedded_geometry(width, cx));
                panel.clone().into_any_element()
            }
            SideResource::Plugin(panel) if panel.read(cx).browser_view().is_some() => {
                panel.clone().into_any_element()
            }
            SideResource::Plugin(panel) => v_flex()
                .size_full()
                .min_h_0()
                .child(
                    crate::header::Header::new("plugin-header", cx)
                        .bordered(false)
                        .child(crate::plugins::Panel::heading(panel, cx)),
                )
                .child(panel.clone())
                .into_any_element(),
            SideResource::Link(link) => v_flex()
                .debug_selector(|| "resource-link-preview".into())
                .size_full()
                .gap_3()
                .child(
                    crate::header::Header::new("link-header", cx)
                        .bordered(false)
                        .child(div().flex_1().child(tr("resource_link_title"))),
                )
                .child(div().px_4().child(link.clone()))
                .child(
                    div()
                        .px_4()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("resource_link_preview")),
                )
                .into_any_element(),
            SideResource::PreviewFiles(state) if state.tabs.open.is_empty() => {
                self.file_explorer(true, None, cx)
            }
            SideResource::PreviewGit(state) if state.tabs.open.is_empty() => {
                self.git_changes(true, None, cx)
            }
            _ => {
                let (content, controls) = if page == Page::Git {
                    (self.git_diff(true, cx), self.git_changes(true, None, cx))
                } else {
                    (
                        self.file_preview(true, cx),
                        self.file_explorer(true, None, cx),
                    )
                };
                split::Split {
                    width,
                    id: format!(
                        "conversation-resource-{}-{}",
                        self.session_scope.active.selector(),
                        if page == Page::Git { "git" } else { "files" }
                    )
                    .into(),
                    content,
                    controls,
                    controls_width: match panel {
                        SideResource::PreviewFiles(state) => Some(state.list_width.clone()),
                        SideResource::PreviewGit(state) => Some(state.list_width.clone()),
                        _ => None,
                    },
                }
                .into_any_element()
            }
        };
        v_flex()
            .id(SharedString::from(format!(
                "panel-{}",
                self.session_scope.active.selector()
            )))
            .debug_selector(|| "resource-side-panel".into())
            .track_focus(&self.panel_focus)
            .key_context("ResourcePanel")
            .on_action(cx.listener(Self::close_focused_resource))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|shell, _, window, cx| {
                    if !shell.panel_focus.contains_focused(window, cx) {
                        shell.panel_focus.focus(window, cx);
                    }
                }),
            )
            .size_full()
            .child(content)
            .into_any_element()
    }
}
