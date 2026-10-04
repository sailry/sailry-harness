use super::*;
use gpui_kit::component::{
    input::Input,
    list::ListItem,
    tab::{Tab, TabBar},
};

impl Shell {
    pub(crate) fn project_records(&self, cx: &mut Context<Self>) -> AnyElement {
        let terminals = self.project_tab == 1;
        let mut records = Vec::new();
        if let Some(live) = &self.live {
            if let (Some(project), Some(snapshot)) = (live.project, &live.view.snapshot) {
                if terminals {
                    for terminal in snapshot.terminals.iter().filter(|terminal| {
                        terminal.status != sailry_protocol::terminal::Status::Closed
                            && snapshot.worktrees.iter().any(|tree| {
                                Some(tree.id) == terminal.worktree && tree.project == Some(project)
                            })
                    }) {
                        let path = terminal
                            .directory
                            .as_ref()
                            .and_then(|value| url::Url::parse(value).ok())
                            .map(|uri| {
                                uri.to_file_path()
                                    .map(|path| path.display().to_string())
                                    .unwrap_or_else(|_| uri.path().to_owned())
                            })
                            .unwrap_or_else(|| {
                                snapshot
                                    .worktrees
                                    .iter()
                                    .find(|tree| Some(tree.id) == terminal.worktree)
                                    .map(|tree| tree.path.clone())
                                    .unwrap_or_default()
                            });
                        records.push((
                            terminal.id.to_string(),
                            self.terminal_title(
                                live.selected,
                                terminal.worktree.unwrap(),
                                terminal.id,
                                cx,
                            ),
                            SharedString::from(path),
                            Box::new(crate::live::menus::Dispatch {
                                node: live.selected,
                                target: crate::live::menus::Target::Terminal(terminal.id),
                                command: crate::live::menus::Command::Open,
                            }) as Box<dyn Action>,
                        ));
                    }
                } else {
                    for session in snapshot.sessions.iter().filter(|session| {
                        session.project == Some(project)
                            && session.config.resource.is_none()
                            && session.delegation.is_none()
                    }) {
                        let path = snapshot
                            .worktrees
                            .iter()
                            .find(|tree| tree.id == session.worktree)
                            .map(|tree| tree.path.clone())
                            .unwrap_or_default();
                        records.push((
                            session.id.to_string(),
                            crate::activity::title(session),
                            SharedString::from(path),
                            Box::new(crate::live::menus::Dispatch {
                                node: live.selected,
                                target: crate::live::menus::Target::Session(session.id),
                                command: crate::live::menus::Command::Open,
                            }) as Box<dyn Action>,
                        ));
                    }
                }
            }
        } else {
            let project = self.workspace.owner(self.host).project;
            if terminals {
                for (&key, terminal) in &self.workspace.terminals {
                    if terminal.owner.project == project {
                        records.push((
                            format!("{}-{}", key.0, key.1),
                            terminal.title.clone(),
                            self.workspace.worktrees[&terminal.owner.worktree]
                                .path
                                .clone(),
                            Box::new(crate::workspace::Dispatch {
                                target: crate::workspace::Target::Terminal(key),
                                command: crate::workspace::Command::Open,
                            }) as Box<dyn Action>,
                        ));
                    }
                }
            } else {
                for (&key, session) in self.workspace.sessions.iter().rev() {
                    if session.owner.project == project {
                        records.push((
                            format!("{}-{}", key.0, key.1),
                            session.title.clone(),
                            self.workspace.worktrees[&session.owner.worktree]
                                .path
                                .clone(),
                            Box::new(crate::workspace::Dispatch {
                                target: crate::workspace::Target::Session(key),
                                command: crate::workspace::Command::Open,
                            }) as Box<dyn Action>,
                        ));
                    }
                }
            }
        }
        let query = self.project_search.read(cx).value().trim().to_lowercase();
        if !query.is_empty() {
            records.retain(|(_, title, path, _)| {
                title.to_lowercase().contains(&query) || path.to_lowercase().contains(&query)
            });
        }
        let icon = if terminals {
            IconName::SquareTerminal
        } else {
            IconName::Bot
        };
        v_flex()
            .w_full()
            .gap_3()
            .mt_4()
            .debug_selector(|| "project-records-list".into())
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_4()
                    .child(
                        TabBar::new("project-record-tabs")
                            .flex_shrink_0()
                            .segmented()
                            .selected_index(self.project_tab)
                            .children([
                                Tab::new()
                                    .label(tr("workspace_sessions"))
                                    .debug_selector(|| "project-record-sessions".into()),
                                Tab::new()
                                    .label(tr("terminal"))
                                    .debug_selector(|| "project-record-terminals".into()),
                            ])
                            .on_click(cx.listener(|shell, &index, _, cx| {
                                shell.project_tab = index;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .w(px(240.))
                            .min_w_0()
                            .debug_selector(|| "project-record-search".into())
                            .child(
                                Input::new(&self.project_search)
                                    .prefix(Icon::new(IconName::Search).size_4())
                                    .cleanable(true)
                                    .aria_label(tr("project_records_search")),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_lg()
                    .p_2()
                    .when(records.is_empty(), |list| {
                        list.child(crate::empty_state::list(
                            icon.clone(),
                            if !query.is_empty() {
                                "project_records_no_results"
                            } else if terminals {
                                "project_terminals_empty"
                            } else {
                                "project_sessions_empty"
                            },
                            cx,
                        ))
                    })
                    .children(records.into_iter().map(|(id, title, path, action)| {
                        let session = self.live.as_ref().and_then(|live| {
                            live.view
                                .snapshot
                                .as_ref()?
                                .sessions
                                .iter()
                                .find(|session| session.id.to_string() == id)
                                .map(|session| (live.selected, session.clone()))
                        });
                        let selector = format!("project-record-{id}");
                        ListItem::new(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .py_3()
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_3()
                                    .child(Icon::new(icon.clone()).size_4().flex_shrink_0())
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(div().truncate().text_sm().child(title))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(path),
                                            ),
                                    )
                                    .when_some(session.as_ref(), |row, (node, session)| {
                                        row.child(self.live_session_actions(*node, session, cx))
                                    }),
                            )
                            .when_some(session, |row, (node, session)| {
                                row.on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |shell, event, window, cx| {
                                        shell.live_resource_menu(
                                            node,
                                            crate::live::menus::Target::Session(session.id),
                                            event,
                                            window,
                                            cx,
                                        );
                                    }),
                                )
                            })
                            .on_click(move |_, window, cx| {
                                window.dispatch_action(action.boxed_clone(), cx)
                            })
                    })),
            )
            .into_any_element()
    }
}
