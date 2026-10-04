use super::{Owner, model::Key};
use crate::theme::DialogStyle as _;
use crate::{preview::Page, shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    native_menu::NativeMenu,
    scroll::ScrollableElement,
    *,
};
use gpui_kit::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Project(Owner),
    Session(Key),
    Terminal(Key),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    NewSession,
    NewTerminal,
    LaunchCli,
    Worktrees,
    SelectWorktree,
    NewWorktree,
    RemoveWorktree,
    EditProject,
    RemoveProject,
    Trust,
    Rename,
    Pin,
    Archive,
    Copy,
    History,
    Reset,
    Close,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = workspace, no_json)]
pub(crate) struct Dispatch {
    pub target: Target,
    pub command: Command,
}

impl Shell {
    pub(crate) fn project_menu(&self, button: Button, cx: &mut Context<Self>) -> impl IntoElement {
        let target = Target::Project(self.workspace.owner(self.host));
        let create_disabled = self
            .workspace
            .new_worktree_reason(self.workspace.owner(self.host).project)
            .is_some();
        let remove_disabled = self
            .workspace
            .removal_reason(self.workspace.owner(self.host))
            .is_some();
        let shell = cx.entity().downgrade();
        button.dropdown_menu(move |menu, _, _| {
            [
                (Command::EditProject, "project_edit", IconName::Settings2),
                (Command::Trust, "project_trust", IconName::CircleCheck),
                (Command::Worktrees, "worktree_manage", IconName::Network),
                (Command::NewWorktree, "worktree_create", IconName::Plus),
                (Command::Copy, "workspace_copy_path", IconName::Copy),
                (
                    Command::RemoveWorktree,
                    "worktree_remove",
                    IconName::CircleX,
                ),
                (Command::RemoveProject, "project_remove", IconName::Delete),
                (Command::LaunchCli, "cli_launch", IconName::SquareTerminal),
            ]
            .into_iter()
            .fold(menu, |menu, (command, label, icon)| {
                let shell = shell.clone();
                menu.item(
                    PopupMenuItem::new(tr(label))
                        .icon(icon)
                        .disabled(
                            (command == Command::RemoveWorktree && remove_disabled)
                                || (command == Command::NewWorktree && create_disabled),
                        )
                        .on_click(move |_, window, cx| {
                            _ = shell.update(cx, |shell, cx| {
                                shell.workspace_action(&Dispatch { target, command }, window, cx)
                            });
                        }),
                )
            })
        })
    }

    pub(crate) fn workspace_menu(
        &mut self,
        target: Target,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.focus.focus(window, cx);
        let menu = self.native_workspace_menu(target);
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            menu.show(event.position, window, cx);
        } else {
            // Kit's Linux backend draws a popup; do not present that as an OS-native menu.
            crate::feedback::error(
                &tr("workspace_native_menu_unavailable"),
                &tr("workspace_native_menu_platform"),
                window,
                cx,
            );
        }
    }

    pub(crate) fn native_workspace_menu(&self, target: Target) -> NativeMenu {
        let items: &[(Command, &str, IconName)] = match target {
            Target::Project(_) => &[
                (Command::Open, "workspace_project", IconName::Folder),
                (Command::NewSession, "workspace_new_session", IconName::Plus),
                (
                    Command::NewTerminal,
                    "workspace_new_terminal",
                    IconName::SquareTerminal,
                ),
                (
                    Command::History,
                    "workspace_records",
                    IconName::GalleryVerticalEnd,
                ),
                (Command::EditProject, "project_edit", IconName::Settings2),
                (Command::Trust, "project_trust", IconName::CircleCheck),
                (Command::Copy, "workspace_copy_path", IconName::Copy),
                (Command::Worktrees, "worktree_manage", IconName::Network),
                (Command::NewWorktree, "worktree_create", IconName::Plus),
                (
                    Command::RemoveWorktree,
                    "worktree_remove",
                    IconName::CircleX,
                ),
                (Command::RemoveProject, "project_remove", IconName::Delete),
                (Command::LaunchCli, "cli_launch", IconName::SquareTerminal),
            ],
            Target::Session(key) => &[
                (Command::Open, "workspace_open_session", IconName::Bot),
                (Command::Rename, "workspace_rename", IconName::Settings2),
                (
                    Command::Pin,
                    if self.workspace.sessions.get(&key).is_some_and(|s| s.pinned) {
                        "workspace_unpin"
                    } else {
                        "workspace_pin"
                    },
                    IconName::Star,
                ),
                (Command::Copy, "workspace_copy_title", IconName::Copy),
                (Command::Archive, "workspace_archive", IconName::Inbox),
            ],
            Target::Terminal(_) => &[
                (
                    Command::Open,
                    "workspace_open_terminal",
                    IconName::SquareTerminal,
                ),
                (Command::Rename, "workspace_rename", IconName::Settings2),
                (
                    Command::Reset,
                    "workspace_reset_terminal",
                    IconName::RotateCw,
                ),
                (Command::Copy, "workspace_copy_path", IconName::Copy),
                (Command::Close, "workspace_close_terminal", IconName::Close),
            ],
        };
        items.iter().enumerate().fold(
            NativeMenu::new(),
            |menu, (index, &(command, label, ref icon))| {
                if let Target::Project(owner) = target {
                    if command == Command::LaunchCli {
                        return menu.submenu(tr(label), self.preview_cli_menu(owner));
                    }
                    if matches!(command, Command::NewWorktree | Command::RemoveWorktree) {
                        return menu;
                    }
                    if command == Command::Worktrees {
                        let mut trees = NativeMenu::new()
                            .menu_with_disabled(
                                tr("worktree_create"),
                                self.workspace.new_worktree_reason(owner.project).is_some(),
                                Box::new(Dispatch {
                                    target,
                                    command: Command::NewWorktree,
                                }),
                            )
                            .separator();
                        for (&id, tree) in &self.workspace.worktrees {
                            if tree.project == owner.project {
                                trees = trees.menu_with_check(
                                    tree.branch.clone(),
                                    owner.worktree == id,
                                    Box::new(Dispatch {
                                        target: Target::Project(Owner {
                                            worktree: id,
                                            ..owner
                                        }),
                                        command: Command::SelectWorktree,
                                    }),
                                );
                            }
                        }
                        if !self.workspace.worktrees[&owner.worktree].main {
                            trees = trees.separator().menu_with_disabled(
                                tr("worktree_remove"),
                                self.workspace.removal_reason(owner).is_some(),
                                Box::new(Dispatch {
                                    target,
                                    command: Command::RemoveWorktree,
                                }),
                            );
                        }
                        return menu.submenu(tr(label), trees);
                    }
                }
                let menu = if index == 1
                    || matches!(
                        command,
                        Command::Archive
                            | Command::Close
                            | Command::RemoveWorktree
                            | Command::RemoveProject
                    ) {
                    menu.separator()
                } else {
                    menu
                };
                menu.menu_with_icon_disabled(
                    tr(label),
                    icon.clone(),
                    match (command, target) {
                        (Command::RemoveWorktree, Target::Project(owner)) => {
                            self.workspace.removal_reason(owner).is_some()
                        }
                        (Command::NewWorktree, Target::Project(owner)) => {
                            self.workspace.new_worktree_reason(owner.project).is_some()
                        }
                        _ => false,
                    },
                    Box::new(Dispatch { target, command }),
                )
            },
        )
    }

    pub(crate) fn workspace_action(
        &mut self,
        action: &Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = match action.target {
            Target::Project(owner) => self.workspace.contains(owner).then_some(owner),
            Target::Session(key) => self.workspace.sessions.get(&key).map(|s| s.owner),
            Target::Terminal(key) => self.workspace.terminals.get(&key).map(|s| s.owner),
        };
        let Some(owner) = owner else {
            return;
        };
        match action.command {
            Command::Open => match action.target {
                Target::Project(_) => self.select_project(owner.project, window, cx),
                Target::Session(key) => {
                    self.workspace.sessions.get_mut(&key).unwrap().archived = false;
                    self.select_session(key, window, cx);
                }
                Target::Terminal(key) => self.select_terminal(key, window, cx),
            },
            Command::NewSession => self.create_session(owner, window, cx),
            Command::NewTerminal => self.create_terminal(owner, window, cx),
            Command::LaunchCli => self.cli_launcher(owner, window, cx),
            Command::Worktrees => self.worktree_manager(owner.project, window, cx),
            Command::SelectWorktree => {
                self.select_owner(owner, window, cx);
                self.navigate(Page::Project, window, cx);
            }
            Command::NewWorktree => self.new_worktree(owner, window, cx),
            Command::RemoveWorktree => self.remove_worktree_dialog(owner, window, cx),
            Command::RemoveProject => self.remove_project_dialog(owner.project, window, cx),
            Command::EditProject => {
                self.project_editor(owner.host, Some(owner.project), window, cx);
            }
            Command::Trust => self.project_trust(owner.project, None, window, cx),
            Command::Rename => self.rename_resource(action.target, window, cx),
            Command::Pin => {
                if let Target::Session(key) = action.target {
                    let session = self.workspace.sessions.get_mut(&key).unwrap();
                    session.pinned = !session.pinned;
                }
            }
            Command::Copy => {
                let value = match action.target {
                    Target::Session(key) => self.workspace.sessions[&key].title.clone(),
                    Target::Project(_) => self.workspace.projects[&owner.project].path.clone(),
                    Target::Terminal(_) => self.workspace.worktrees[&owner.worktree].path.clone(),
                };
                cx.write_to_clipboard(ClipboardItem::new_string(value.to_string()));
            }
            Command::History => self.session_records(owner.project, window, cx),
            Command::Archive | Command::Close | Command::Reset => {
                self.confirm_resource(action.clone(), window, cx);
            }
        }
        cx.notify();
    }

    fn rename_resource(&self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let value = match target {
            Target::Project(owner) => self.workspace.projects[&owner.project].name.clone(),
            Target::Session(key) => self.workspace.sessions[&key].title.clone(),
            Target::Terminal(key) => self.workspace.terminals[&key].title.clone(),
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value)
                .placeholder(tr("form_name_hint"))
        });
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _| {
            let input = input.clone();
            let owner = owner.clone();
            dialog
                .form_title(tr("workspace_rename"))
                .w((window.viewport_size().width - px(48.)).min(px(420.)))
                .child(
                    div()
                        .debug_selector(|| "workspace-name-input".into())
                        .child(Input::new(&input).aria_label(tr("workspace_name"))),
                )
                .child(div().text_sm().child(tr("workspace_rename_hint")))
                .footer(
                    gpui_kit::component::dialog::DialogFooter::new()
                        .w_full()
                        .gap_2()
                        .child(
                            Button::new("rename-cancel")
                                .label(tr("settings_cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("rename-save")
                                .primary()
                                .label(tr("settings_save_preview"))
                                .debug_selector(|| "workspace-rename-save".into())
                                .on_click(move |_, window, cx| {
                                    let name: SharedString =
                                        input.read(cx).value().trim().to_owned().into();
                                    if name.is_empty() {
                                        return;
                                    }
                                    _ = owner.update(cx, |this, cx| {
                                        match target {
                                            Target::Project(owner) => {
                                                if let Some(project) =
                                                    this.workspace.projects.get_mut(&owner.project)
                                                {
                                                    project.name = name.clone();
                                                }
                                            }
                                            Target::Session(key) => {
                                                if let Some(session) =
                                                    this.workspace.sessions.get_mut(&key)
                                                {
                                                    session.title = name.clone();
                                                }
                                            }
                                            Target::Terminal(key) => {
                                                if let Some(terminal) =
                                                    this.workspace.terminals.get_mut(&key)
                                                {
                                                    terminal.title = name.clone();
                                                }
                                            }
                                        }
                                        cx.notify();
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }

    fn confirm_resource(&self, action: Dispatch, window: &mut Window, cx: &mut Context<Self>) {
        if action.command == Command::Reset
            && let Target::Terminal(key) = action.target
            && let Some(terminal) = self.workspace.terminals.get(&key)
            && terminal.profile.is_some()
            && !self.workspace.projects[&terminal.owner.project].trusted
        {
            self.project_trust(terminal.owner.project, None, window, cx);
            return;
        }
        let owner = cx.entity().downgrade();
        crate::prompts::confirm(
            &tr(match action.command {
                Command::Archive => "workspace_archive",
                Command::Reset => "workspace_reset_terminal",
                _ => "workspace_close_terminal",
            }),
            &tr("workspace_memory_only"),
            tr("workspace_confirm"),
            window,
            cx,
            move |window, cx| {
                let _ = owner.update(cx, |this, cx| this.finish_resource(&action, window, cx));
            },
        );
    }

    fn finish_resource(&mut self, action: &Dispatch, window: &mut Window, cx: &mut Context<Self>) {
        match (action.target, action.command) {
            (Target::Session(key), Command::Archive) => {
                let Some(session) = self.workspace.sessions.get_mut(&key) else {
                    return;
                };
                session.archived = true;
                let project = session.owner.project;
                if self.page == Page::Conversation && (self.host, self.session) == key {
                    self.close_resource_panel(cx);
                    self.select_project(project, window, cx);
                }
            }
            (Target::Terminal(key), Command::Close) => {
                let Some(terminal) = self.workspace.terminals.remove(&key) else {
                    return;
                };
                if self.workspace.terminal == Some(key) {
                    self.workspace.terminal = None;
                    if self.page == Page::Terminal {
                        self.select_project(terminal.owner.project, window, cx);
                    }
                }
            }
            (Target::Terminal(key), Command::Reset) => {
                if let Some(terminal) = self.workspace.terminals.get_mut(&key)
                    && (terminal.profile.is_none()
                        || self.workspace.projects[&terminal.owner.project].trusted)
                {
                    terminal.resets += 1;
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn session_records(
        &self,
        project: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let records: Vec<_> = self
            .workspace
            .sessions
            .iter()
            .filter(|(_, s)| s.owner.project == project)
            .map(|(&key, s)| (key, s.title.clone(), s.archived))
            .collect();
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .form_title(tr("workspace_records"))
                .w((window.viewport_size().width - px(48.)).min(px(560.)))
                .child(
                    v_flex()
                        .id("session-records")
                        .max_h(px(360.))
                        .overflow_y_scrollbar()
                        .gap_1()
                        .children(records.iter().map(|(key, title, archived)| {
                            let key = *key;
                            let owner = owner.clone();
                            Button::new(("session-record", key.1))
                                .ghost()
                                .w_full()
                                .debug_selector(move || format!("session-record-{}", key.1))
                                .icon(if *archived {
                                    IconName::Inbox
                                } else {
                                    IconName::Bot
                                })
                                .label(title.clone())
                                .tooltip(tr(if *archived {
                                    "workspace_restore"
                                } else {
                                    "workspace_open_session"
                                }))
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = owner.update(cx, |this, cx| {
                                        if let Some(session) = this.workspace.sessions.get_mut(&key)
                                        {
                                            session.archived = false;
                                        }
                                        this.sidebar.open_project(project);
                                        this.select_session(key, window, cx);
                                    });
                                })
                        })),
                )
        });
    }
}
