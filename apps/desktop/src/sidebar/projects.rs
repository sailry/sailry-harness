use super::*;
use crate::workspace::Target;
use gpui_kit::component::collapsible::Collapsible;

impl Shell {
    pub(super) fn project_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let empty = self.live.as_ref().map_or_else(
            || {
                self.workspace
                    .projects
                    .values()
                    .all(|project| project.host != self.host)
            },
            |live| {
                live.view.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.node == live.selected && snapshot.projects.is_empty()
                })
            },
        );
        if empty {
            return div()
                .debug_selector(|| "sidebar-projects-empty".into())
                .px_2()
                .py_1()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr("connection_no_projects"))
                .into_any_element();
        }
        if self.live.is_some() {
            return self.live_project_rows(cx).into_any_element();
        }
        v_flex()
            .gap_0p5()
            .children(
                self.workspace
                    .projects
                    .iter()
                    .filter(|(_, project)| project.host == self.host)
                    .map(|(&id, project)| {
                        let owner = self.workspace.project_owner(id).unwrap();
                        let open = !self.sidebar.closed_projects.contains(&id);
                        let selected = self.page == Page::Project
                            && self
                                .workspace
                                .selected_owner(self.host)
                                .is_some_and(|owner| owner.project == id);
                        let mut sessions: Vec<_> = self
                            .workspace
                            .sessions
                            .iter()
                            .filter(|(_, session)| session.owner.project == id && !session.archived)
                            .collect();
                        sessions.sort_by_key(|(key, session)| {
                            (!session.pinned, std::cmp::Reverse(**key))
                        });
                        let last_session = sessions.last().map(|(key, _)| **key);
                        let last_terminal = self
                            .workspace
                            .terminals
                            .iter()
                            .rev()
                            .find(|(_, terminal)| terminal.owner.project == id)
                            .map(|(key, _)| *key);

                        let has_resources = !sessions.is_empty()
                            || self
                                .workspace
                                .terminals
                                .values()
                                .any(|terminal| terminal.owner.project == id);
                        Collapsible::new()
                            .open(open)
                            .child(
                                div()
                                    .id(("project-context", id))
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(move |this, event, window, cx| {
                                            this.workspace_menu(
                                                Target::Project(owner),
                                                event,
                                                window,
                                                cx,
                                            );
                                        }),
                                    )
                                    .child(
                                        self.sidebar_row(
                                            ("project", id),
                                            Row::Project(id),
                                            selected,
                                            cx,
                                        )
                                        .debug_selector(move || {
                                            if id == 0 {
                                                "sidebar-project".into()
                                            } else {
                                                format!("sidebar-project-{id}")
                                            }
                                        })
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .w_full()
                                                .child(crate::workspace::appearance::project(
                                                    &project.appearance,
                                                    cx,
                                                ))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .truncate()
                                                        .child(project.name.clone()),
                                                )
                                                .child(
                                                    Button::new(("project-disclosure", id))
                                                        .ghost()
                                                        .xsmall()
                                                        .when(
                                                            self.sidebar.hovered
                                                                != Some(Row::Project(id)),
                                                            |button| {
                                                                button.opacity(0.).focus(|style| {
                                                                    style.opacity(1.)
                                                                })
                                                            },
                                                        )
                                                        .debug_selector(move || {
                                                            format!("project-disclosure-{id}")
                                                        })
                                                        .icon(if open {
                                                            IconName::ChevronDown
                                                        } else {
                                                            IconName::ChevronRight
                                                        })
                                                        .tooltip(tr(if open {
                                                            "workspace_collapse"
                                                        } else {
                                                            "workspace_expand"
                                                        }))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                cx.stop_propagation();
                                                                if !this
                                                                    .sidebar
                                                                    .closed_projects
                                                                    .remove(&id)
                                                                {
                                                                    this.sidebar
                                                                        .closed_projects
                                                                        .insert(id);
                                                                }
                                                                cx.notify();
                                                            },
                                                        )),
                                                ),
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                if !this.sidebar.closed_projects.remove(&id) {
                                                    this.sidebar.closed_projects.insert(id);
                                                }
                                                cx.notify();
                                            }),
                                        ),
                                    ),
                            )
                            .content(
                                v_flex()
                                    .debug_selector(move || format!("project-resources-{id}"))
                                    .gap_0p5()
                                    .when(has_resources, |body| body.py_0p5())
                                    .children(sessions.into_iter().map(|(&key, session)| {
                                        let row = Row::Session(key.0, key.1);
                                        let active = self.page == Page::Conversation
                                            && (self.host, self.session) == key;
                                        let title = session.title.clone();
                                        let branch = self.workspace.branch_label(session.owner);
                                        let hovered = self.sidebar.hovered == Some(row);
                                        let worktree = self.workspace.worktrees
                                            .get(&session.owner.worktree)
                                            .is_some_and(|tree| !tree.main);
                                        div()
                                            .id(("session-context", key.1))
                                            .on_mouse_down(
                                                MouseButton::Right,
                                                cx.listener(move |this, event, window, cx| {
                                                    this.workspace_menu(
                                                        Target::Session(key),
                                                        event,
                                                        window,
                                                        cx,
                                                    );
                                                }),
                                            )
                                            .child(
                                                self.sidebar_row(
                                                    ("session", key.1),
                                                    row,
                                                    active,
                                                    cx,
                                                )
                                                .pl(px(30.))
                                                .relative()
                                                .child(super::branch(
                                                    format!("session-{}", key.1),
                                                    last_terminal.is_none()
                                                        && last_session == Some(key),
                                                    px(30.),
                                                    cx,
                                                ))
                                                .debug_selector(move || {
                                                    format!("session-{}", key.1)
                                                })
                                                .child(
                                                    h_flex()
                                                        .w_full()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .when(session.pinned, |row| {
                                                            row.child(
                                                                Icon::new(IconName::Star)
                                                                    .size_3()
                                                                    .mr_1(),
                                                            )
                                                        })
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .truncate()
                                                                .child(title),
                                                        )
                                                        .when(hovered, |row| {
                                                            row.child(super::location(
                                                                format!("session-branch-{}", key.1),
                                                                branch,
                                                                cx,
                                                            ))
                                                        })
                                                        .when(worktree, |row| {
                                                            row.child(
                                                                div()
                                                                    .flex_shrink_0()
                                                                    .ml_2()
                                                                    .debug_selector(move || {
                                                                        format!("session-worktree-{}", key.1)
                                                                    })
                                                                    .child(
                                                                        Icon::new(IconName::Network)
                                                                            .size_4()
                                                                            .text_color(cx.theme().muted_foreground),
                                                                    ),
                                                            )
                                                        }),
                                                )
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.select_session(key, window, cx)
                                                    },
                                                )),
                                            )
                                    }))
                                    .children(
                                        self.workspace
                                            .terminals
                                            .iter()
                                            .filter(|(_, terminal)| terminal.owner.project == id)
                                            .map(|(&key, terminal)| {
                                                div()
                                                    .id(("terminal-context", key.1))
                                                    .on_mouse_down(
                                                        MouseButton::Right,
                                                        cx.listener(
                                                            move |this, event, window, cx| {
                                                                this.workspace_menu(
                                                                    Target::Terminal(key),
                                                                    event,
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        ),
                                                    )
                                                    .child(
                                                        self.sidebar_row(
                                                            ("terminal", key.1),
                                                            Row::Terminal(key.0, key.1),
                                                            self.page == Page::Terminal
                                                                && self.workspace.terminal
                                                                    == Some(key),
                                                            cx,
                                                        )
                                                        .pl(px(30.))
                                                        .relative()
                                                        .child(super::branch(
                                                            format!("terminal-{}", key.1),
                                                            last_terminal == Some(key),
                                                            px(30.),
                                                            cx,
                                                        ))
                                                        .debug_selector(move || {
                                                            format!("terminal-{}", key.1)
                                                        })
                                                        .child(
                                                            h_flex()
                                                                .w_full()
                                                                .min_w_0()
                                                                .gap_2()
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .truncate()
                                                                        .child(terminal.title.clone()),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex_shrink_0()
                                                                        .debug_selector(move || format!("terminal-icon-{}", key.1))
                                                                        .child(
                                                                            Icon::new(IconName::SquareTerminal)
                                                                                .size_4()
                                                                                .text_color(cx.theme().muted_foreground),
                                                                        ),
                                                                ),
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                this.select_terminal(
                                                                    key, window, cx,
                                                                )
                                                            },
                                                        )),
                                                    )
                                            }),
                                    ),
                            )
                    }),
            )
            .into_any_element()
    }
}
