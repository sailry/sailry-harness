//! Link routing keeps paths attached to their originating execution Node.
use super::*;
use crate::{
    backend::Services,
    conversation::{
        file_reference::{self, Target},
        links::{self, Mode, Opening, Route},
        live::View as Chat,
    },
};
use sailry_protocol::{NodeId, WorktreeId};

impl Shell {
    pub(crate) fn open_conversation_link(
        &mut self,
        link: SharedString,
        mode: Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let link = SharedString::from(links::normalize(&link));
        // Preview links never leave the preview.
        let mode = if self.live.is_some() {
            mode
        } else {
            Mode::Internal
        };
        match links::route(&link, mode) {
            Route::Browser => {
                let target = link.clone();
                if self.guard_file_navigation(window, cx, move |shell, window, cx| {
                    shell.open_conversation_link(target.clone(), Mode::Internal, window, cx)
                }) {
                    return;
                }
                self.open_browser_resource(Some(link.to_string()), window, cx);
                cx.notify();
                return;
            }
            Route::System if self.live.is_some() => {
                cx.open_url(&link);
                return;
            }
            Route::System | Route::Resource => {}
        }
        if let Some(chat) = self.current_chat().cloned() {
            self.open_chat_link(&chat, None, link, mode, window, cx);
            return;
        }
        if knowledge::recognizes(&link) {
            knowledge::failure("knowledge_citation_unavailable", window, cx);
            return;
        }
        if let Some(live) = &self.live {
            let located = live
                .selected_worktree()
                .zip(
                    live.view
                        .snapshot
                        .as_ref()
                        .filter(|snapshot| snapshot.node == live.selected),
                )
                .and_then(|(worktree, snapshot)| {
                    file_reference::locate(&link, worktree.id, &snapshot.worktrees)
                });
            let Some((worktree, target)) = located else {
                crate::resources::file_open::failure("files_open_failed", window, cx);
                return;
            };
            let node = live.selected;
            self.open_workspace_link((node, worktree), target, link, mode, window, cx);
            return;
        }
        let path = link.split(['#', '?']).next().unwrap_or(&link);
        if let Some(file) = FILES
            .iter()
            .position(|file| path == *file || path == format!("./{file}"))
        {
            self.open_resource_panel(Page::Files, window, cx);
            self.file_state_mut(true).tabs.open(file);
        } else {
            // Unresolved links stay inside the preview; never open a browser or read a real file.
            let target = link.clone();
            if self.guard_file_navigation(window, cx, move |shell, window, cx| {
                shell.open_conversation_link(target.clone(), Mode::Internal, window, cx)
            }) {
                return;
            }
            self.side_resource = Some(crate::resources::SideResource::Link(link));
            self.layout.panel_open[0] = true;
        }
        cx.notify();
    }

    pub(crate) fn open_chat_link(
        &mut self,
        chat: &Entity<Chat>,
        origin: Option<WorktreeId>,
        link: SharedString,
        mode: Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let link = SharedString::from(links::normalize(&link));
        match links::route(&link, mode) {
            Route::Browser => {
                self.open_conversation_link(link, mode, window, cx);
                return;
            }
            Route::System => {
                cx.open_url(&link);
                return;
            }
            Route::Resource => {}
        }
        if knowledge::recognizes(&link) {
            self.open_knowledge_link(chat, link, window, cx);
            return;
        }
        let view = chat.read(cx);
        let binding = view.binding();
        let Some(worktree) = origin.or(binding.worktree) else {
            crate::resources::file_open::failure("files_open_failed", window, cx);
            return;
        };
        let snapshot = view.node_snapshot().or_else(|| {
            self.live
                .as_ref()
                .and_then(|live| live.view.snapshot.as_ref())
                .filter(|snapshot| snapshot.node == binding.client.target())
        });
        let target = snapshot
            .and_then(|snapshot| file_reference::locate(&link, worktree, &snapshot.worktrees));
        let Some((worktree, target)) = target else {
            crate::resources::file_open::failure("files_open_failed", window, cx);
            return;
        };
        self.open_workspace_link(
            (binding.client.target(), worktree),
            target,
            link,
            mode,
            window,
            cx,
        );
    }

    fn open_workspace_link(
        &mut self,
        scope: (NodeId, WorktreeId),
        target: Target,
        link: SharedString,
        mode: Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let local = cx.global::<Services>().local.target() == scope.0;
        match (
            links::opening(&target, link.ends_with('/'), local, mode),
            target,
        ) {
            (Opening::Folder, Target::Workspace { path, .. }) => {
                self.open_file_in_system(scope, path, true, window, cx);
            }
            // Confined to the worktree; a local Node needs no file transfer.
            (Opening::System, Target::Workspace { path, .. }) => {
                self.open_file_in_system(scope, path, false, window, cx);
            }
            (Opening::Document, Target::Workspace { path, line }) => {
                self.open_conversation_file(scope, path, line, window, cx);
            }
            (Opening::System, Target::External(path)) => {
                crate::resources::file_open::local(path, window, cx);
            }
            _ => crate::resources::file_open::failure("files_remote_path", window, cx),
        }
    }
}
