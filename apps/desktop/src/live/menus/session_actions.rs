//! Session context actions use captured identities and existing workflow owners.
use super::*;
use crate::preferences::sessions;
use sailry_protocol::Session;

impl Shell {
    pub(super) fn session_menu(
        &self,
        session: &Session,
        window: &Window,
        cx: &mut App,
    ) -> NativeMenu {
        let live = self.live.as_ref().unwrap();
        let node = live.selected;
        let state = sessions::get(node, session.id, cx);
        let busy = session.activity.run.as_ref().is_some_and(|run| {
            matches!(
                run.status,
                sailry_protocol::conversation::Status::Queued
                    | sailry_protocol::conversation::Status::Running
                    | sailry_protocol::conversation::Status::Stopping
            )
        }) || session.activity.queued > 0;
        let has_tree = live.view.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .worktrees
                .iter()
                .any(|tree| tree.id == session.worktree)
        });
        let disabled = |command| match command {
            Command::CopyPath => !has_tree,
            Command::OpenDirectory => !has_tree || node != live.services.local.target(),
            Command::Fork => busy || session.project.is_none(),
            Command::ForkWorktree => {
                busy || session.project.is_none()
                    || !self.contribution_declared(
                        sailry_protocol::plugin::ui::Intent::ForkWorktree,
                        cx,
                    )
            }
            _ => false,
        };
        #[cfg(test)]
        let actions = std::cell::RefCell::new(Vec::new());
        let action = |command| {
            let action = Dispatch {
                node,
                target: Target::Session(session.id),
                command,
            };
            #[cfg(test)]
            actions
                .borrow_mut()
                .push((action.clone(), disabled(command)));
            Box::new(action)
        };
        let copy = NativeMenu::new()
            .menu(tr("workspace_copy_title"), action(Command::Copy))
            .menu(tr("session_copy_id"), action(Command::CopyId))
            .menu_with_disabled(
                tr("workspace_copy_path"),
                disabled(Command::CopyPath),
                action(Command::CopyPath),
            );
        let fork = NativeMenu::new()
            .menu_with_disabled(
                tr("session_fork_here"),
                disabled(Command::Fork),
                action(Command::Fork),
            )
            .menu_with_disabled(
                tr("session_fork_worktree"),
                disabled(Command::ForkWorktree),
                action(Command::ForkWorktree),
            );
        let menu = NativeMenu::new()
            .menu_with_icon(
                tr("workspace_rename"),
                IconName::Settings2,
                action(Command::Rename),
            )
            .menu_with_icon(
                tr(if state.pinned {
                    "workspace_unpin"
                } else {
                    "workspace_pin"
                }),
                IconName::Star,
                action(if state.pinned {
                    Command::Unpin
                } else {
                    Command::Pin
                }),
            )
            .menu_with_icon(
                tr(if self.session_unread(node, session.id) {
                    "session_mark_read"
                } else {
                    "session_mark_unread"
                }),
                IconName::Eye,
                action(if self.session_unread(node, session.id) {
                    Command::MarkRead
                } else {
                    Command::MarkUnread
                }),
            )
            .separator()
            .menu_with_icon(
                tr(if session.archived {
                    "session_unarchive"
                } else {
                    "workspace_archive"
                }),
                IconName::Inbox,
                action(if session.archived {
                    Command::Unarchive
                } else {
                    Command::Archive
                }),
            )
            .menu_with_icon(
                tr("session_delete"),
                IconName::Delete,
                action(Command::Delete),
            )
            .separator()
            .submenu(tr("content_copy"), copy)
            .separator()
            .submenu(tr("session_fork"), fork)
            .separator()
            .menu_with_icon_disabled(
                tr("files_open"),
                IconName::FolderOpen,
                disabled(Command::OpenDirectory),
                action(Command::OpenDirectory),
            )
            .menu_with_icon(
                tr("session_new_window"),
                IconName::ExternalLink,
                action(Command::NewWindow),
            );
        #[cfg(test)]
        capture::record(window, cx, actions.into_inner());
        #[cfg(not(test))]
        let _ = window;
        menu
    }

    pub(super) fn session_action(
        &mut self,
        command: Command,
        session: &Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let live = self.live.as_ref().unwrap();
        let node = live.selected;
        match command {
            Command::Rename => rename::open(Arc::new(live.client()), session, window, cx),
            Command::Pin | Command::Unpin | Command::MarkRead | Command::MarkUnread => {
                let mut state = sessions::get(node, session.id, cx);
                match command {
                    Command::Pin => state.pinned = true,
                    Command::Unpin => state.pinned = false,
                    Command::MarkUnread => {
                        self.set_session_read(node, session.id, false, window, cx)
                    }
                    Command::MarkRead => {
                        self.mark_session_read(node, session.id, window, cx);
                    }
                    _ => unreachable!(),
                }
                sessions::set(node, session.id, state, cx);
                if let Some(error) = cx.global::<crate::preferences::Preferences>().error {
                    crate::feedback::toast(window, tr(error), Notification::error(tr(error)), cx);
                }
            }
            Command::CopyId => {
                cx.write_to_clipboard(ClipboardItem::new_string(session.id.to_string()))
            }
            Command::CopyPath => {
                if let Some(tree) = live
                    .view
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.worktrees.iter().find(|t| t.id == session.worktree))
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(tree.path.clone()));
                }
            }
            Command::OpenDirectory => {
                self.open_file_in_system((node, session.worktree), String::new(), true, window, cx)
            }
            Command::Fork => self.change_live_session(session, command, window, cx),
            Command::ForkWorktree => {
                if session.project.is_none()
                    || !self.contribution_declared(
                        sailry_protocol::plugin::ui::Intent::ForkWorktree,
                        cx,
                    )
                {
                    return true;
                }
                let deferred = session.clone();
                if self.guard_file_navigation(window, cx, move |shell, window, cx| {
                    if shell
                        .live
                        .as_ref()
                        .is_some_and(|live| live.selected == node)
                    {
                        shell.session_action(Command::ForkWorktree, &deferred, window, cx);
                    }
                }) {
                    return true;
                }
                self.reveal_session(session.clone(), window, cx);
                if let Some(chat) = self.current_chat() {
                    chat.update(cx, |chat, cx| chat.fork_worktree(window, cx));
                }
            }
            Command::NewWindow => self.session_window(session.clone(), window, cx),
            _ => return false,
        }
        cx.notify();
        true
    }
}
