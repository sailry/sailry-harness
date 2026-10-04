//! Link routing keeps paths attached to their originating execution Node.
use super::*;
use crate::{
    backend::Services,
    conversation::{
        file_reference::{self, Target},
        live::View as Chat,
    },
};
use sailry_protocol::{NodeId, WorktreeId};

impl Shell {
    pub(crate) fn open_conversation_link(
        &mut self,
        link: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if url::Url::parse(&link).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) {
            let target = link.clone();
            if self.guard_file_navigation(window, cx, move |shell, window, cx| {
                shell.open_conversation_link(target.clone(), window, cx)
            }) {
                return;
            }
            self.open_browser_resource(Some(link.to_string()), window, cx);
            cx.notify();
            return;
        }
        if let Some(chat) = self.current_chat().cloned() {
            self.open_chat_link(&chat, link, window, cx);
            return;
        }
        if let Some(live) = &self.live {
            if let Some(worktree) = live.selected_worktree() {
                self.open_workspace_link(
                    (live.selected, worktree.id),
                    worktree.path.clone(),
                    link,
                    window,
                    cx,
                );
            }
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
                shell.open_conversation_link(target.clone(), window, cx)
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
        link: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if url::Url::parse(&link).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) {
            self.open_conversation_link(link, window, cx);
            return;
        }
        let view = chat.read(cx);
        let binding = view.binding();
        let Some(worktree) = binding.worktree else {
            return;
        };
        let root = view.worktree_path().map(str::to_owned).or_else(|| {
            self.live
                .as_ref()?
                .view
                .snapshot
                .as_ref()
                .filter(|snapshot| snapshot.node == binding.client.target())?
                .worktrees
                .iter()
                .find(|entry| entry.id == worktree)
                .map(|entry| entry.path.clone())
        });
        let Some(root) = root else {
            return;
        };
        self.open_workspace_link((binding.client.target(), worktree), root, link, window, cx);
    }

    fn open_workspace_link(
        &mut self,
        scope: (NodeId, WorktreeId),
        root: String,
        link: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match file_reference::resolve(&link, &root) {
            Some(Target::Workspace { path, .. }) if path.is_empty() || link.ends_with('/') => {
                self.open_file_in_system(scope, path, true, window, cx);
            }
            Some(Target::Workspace { path, line }) => {
                self.open_conversation_file(scope, path, line, window, cx);
            }
            Some(Target::External(path)) if cx.global::<Services>().local.target() == scope.0 => {
                crate::resources::file_open::local(path, window, cx);
            }
            Some(Target::External(_)) => {
                crate::resources::file_open::failure("files_remote_path", window, cx)
            }
            None => crate::resources::file_open::failure("files_open_failed", window, cx),
        }
    }
}
