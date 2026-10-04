//! Real resource menus dispatch captured Node/resource identities, never preview indices.
use super::*;
use crate::shell::session_scope::Key;
use gpui_kit::component::{native_menu::NativeMenu, notification::Notification, *};
use sailry_protocol::{SessionId, TerminalId};

#[cfg(test)]
mod capture;
mod project;
mod rename;
mod session;
mod session_actions;
#[cfg(test)]
mod tests;
mod window;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Host,
    Project(ProjectId),
    Session(SessionId),
    Terminal(TerminalId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    Rename,
    Pin,
    Unpin,
    MarkUnread,
    MarkRead,
    CopyId,
    CopyPath,
    Fork,
    ForkWorktree,
    OpenDirectory,
    NewWindow,
    AddProject,
    Ports,
    OpenPort {
        port: u16,
    },
    NewSession,
    #[cfg(test)]
    Branches,
    Worktrees,
    Edit,
    Remove,
    Copy,
    Close,
    Revoke,
    Archive,
    Unarchive,
    Delete,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = live_resource, no_json)]
pub(crate) struct Dispatch {
    pub node: NodeId,
    pub target: Target,
    pub command: Command,
}

pub(super) fn items(target: Target, local: bool) -> Vec<(Command, &'static str)> {
    match target {
        Target::Host => {
            let mut items = vec![
                (Command::Open, "workspace_host"),
                (Command::AddProject, "project_add"),
            ];
            if !local {
                items.push((Command::Revoke, "live_revoke"));
            }
            items
        }
        Target::Project(_) => vec![
            (Command::Open, "workspace_project"),
            (Command::Edit, "project_edit"),
            (Command::NewSession, "workspace_new_session"),
            (Command::Copy, "workspace_copy_path"),
            (Command::Remove, "project_remove"),
        ],
        Target::Session(_) => session::items(false),
        Target::Terminal(_) => vec![
            (Command::Open, "workspace_open_terminal"),
            (Command::Copy, "workspace_copy_path"),
            (Command::Close, "workspace_close_terminal"),
        ],
    }
}

impl Command {
    fn available(self, snapshot: Option<&sailry_protocol::Snapshot>) -> bool {
        let _ = snapshot;
        true
    }
}

impl Shell {
    pub(crate) fn live_resource_menu(
        &mut self,
        node: NodeId,
        target: Target,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.focus.focus(window, cx);
        if !cfg!(any(target_os = "macos", target_os = "windows")) {
            crate::feedback::toast(
                window,
                tr("workspace_native_menu_platform"),
                Notification::error(tr("workspace_native_menu_platform")),
                cx,
            );
            return;
        }
        let Some(live) = &self.live else { return };
        if let Target::Project(project) = target {
            if live.selected == node {
                self.show_live_project_menu(project, event.position, window, cx);
            }
            return;
        }
        let local = node == live.services.local.target();
        if let Target::Session(id) = target {
            if live.selected != node {
                return;
            }
            let Some(session) = live.view.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .sessions
                    .iter()
                    .find(|session| session.id == id && session.delegation.is_none())
            }) else {
                return;
            };
            self.session_menu(session, window, cx)
                .show(event.position, window, cx);
            return;
        }
        let items = items(target, local);
        let menu = items
            .into_iter()
            .filter(|(command, _)| *command != Command::Revoke)
            .fold(NativeMenu::new(), |menu, (command, label)| {
                let menu = if matches!(
                    command,
                    Command::AddProject | Command::Copy | Command::Close
                ) {
                    menu.separator()
                } else {
                    menu
                };
                menu.menu(
                    tr(label),
                    Box::new(Dispatch {
                        node,
                        target,
                        command,
                    }),
                )
            });
        let menu = if target == Target::Host {
            let menu = menu
                .separator()
                .submenu(tr("port_mappings"), self.port_menu(node, cx));
            if local {
                menu
            } else {
                menu.separator().menu(
                    tr("live_revoke"),
                    Box::new(Dispatch {
                        node,
                        target,
                        command: Command::Revoke,
                    }),
                )
            }
        } else {
            menu
        };
        menu.show(event.position, window, cx);
    }

    pub(crate) fn live_resource_action(
        &mut self,
        action: &Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.live else { return };
        if matches!(action.target, Target::Host) {
            if live.transport_for(action.node).is_none() {
                return;
            }
            if let Command::OpenPort { port } = action.command {
                self.open_forwarded_port(action.node, port, cx);
            } else if action.command == Command::Ports {
                self.show_host_ports(action.node, window, cx);
            } else if action.command == Command::Revoke {
                self.revoke_device(action.node, window, cx);
            } else {
                let deferred = action.clone();
                if self.guard_file_navigation(window, cx, move |shell, window, cx| {
                    shell.live_resource_action(&deferred, window, cx)
                }) {
                    return;
                }
                self.select_live_host(action.node, window, cx);
                if action.command == Command::AddProject {
                    self.project_editor(0, None, window, cx);
                }
            }
            return;
        }
        // A menu opened on one Node must never operate on a newer selection.
        if live.selected != action.node {
            return;
        }
        let Some(snapshot) = live.view.snapshot.clone() else {
            return;
        };
        if !action.command.available(Some(&snapshot)) {
            return;
        }
        if let Target::Session(id) = action.target {
            let Some(session) = snapshot
                .sessions
                .iter()
                .find(|s| s.id == id && s.delegation.is_none())
                .cloned()
            else {
                return;
            };
            if self.session_action(action.command, &session, window, cx) {
                return;
            }
        }
        if action.command == Command::Copy {
            let value = match action.target {
                Target::Project(id) => snapshot
                    .projects
                    .iter()
                    .find(|value| value.id == id)
                    .map(|p| p.path.clone()),
                Target::Session(id) => snapshot
                    .sessions
                    .iter()
                    .find(|value| value.id == id)
                    .map(|s| crate::activity::title(s).to_string()),
                Target::Terminal(id) => snapshot
                    .terminals
                    .iter()
                    .find(|value| value.id == id)
                    .and_then(|t| {
                        snapshot
                            .worktrees
                            .iter()
                            .find(|tree| Some(tree.id) == t.worktree)
                    })
                    .map(|tree| tree.path.clone()),
                Target::Host => None,
            };
            if let Some(value) = value {
                cx.write_to_clipboard(ClipboardItem::new_string(value));
            }
            return;
        }
        if let Target::Project(project) = action.target {
            let intent = match action.command {
                command if command == Command::Worktrees => {
                    Some(sailry_protocol::plugin::ui::Intent::Worktrees)
                }
                #[cfg(test)]
                Command::Branches => Some(sailry_protocol::plugin::ui::Intent::GitBranches),
                _ => None,
            };
            if let Some(intent) = intent {
                self.project_contribution(project, intent, window, cx);
                return;
            }
        }
        let deferred = action.clone();
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.live_resource_action(&deferred, window, cx)
        }) {
            return;
        }
        let snapshot = self.live.as_ref().unwrap().view.snapshot.clone().unwrap();
        match action.target {
            Target::Project(id) => {
                if !snapshot.projects.iter().any(|p| p.id == id) {
                    return;
                }
                self.select_live_project(id, window, cx);
                match action.command {
                    Command::Edit => {
                        self.live_project_editor(id, window, cx);
                    }
                    Command::Remove => self.remove_live_project(id, window, cx),
                    Command::NewSession => self.new_live_conversation(window, cx),
                    _ => {}
                }
            }
            Target::Session(id) => {
                if let Some(session) = snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == id && s.delegation.is_none())
                {
                    match action.command {
                        Command::Archive | Command::Unarchive | Command::Delete => {
                            self.change_live_session(session, action.command, window, cx);
                        }
                        Command::Open => self.reveal_session(session.clone(), window, cx),
                        _ => {}
                    }
                }
            }
            Target::Terminal(id) => {
                if let Some(terminal) = snapshot.terminals.iter().find(|t| t.id == id)
                    && let Some(tree) = snapshot
                        .worktrees
                        .iter()
                        .find(|tree| Some(tree.id) == terminal.worktree)
                {
                    if let Some(project) = tree.project {
                        self.live
                            .as_mut()
                            .unwrap()
                            .worktree_choices
                            .insert((action.node, project), tree.id);
                        self.select_live_project(project, window, cx);
                    }
                    self.reveal_terminal(tree.project, terminal, window, cx);
                    if action.command == Command::Close {
                        self.terminal_action(Some(id), window, cx);
                    }
                }
            }
            Target::Host => {}
        }
        cx.notify();
    }
}
