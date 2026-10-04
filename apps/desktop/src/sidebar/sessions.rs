//! Session rows keep inline location metadata, navigation and menus on the Kit ListItem.
//! Hover metadata restores f4d8317d's layout, removed in 3db2cc5f, alongside current status rows.
use super::*;
use crate::shell::session_scope::Key;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, Output, SessionId};

pub(crate) enum Placement<'a> {
    Project(&'a super::tree::Entry),
    Recent,
    Active,
}

#[derive(Default)]
pub(super) struct Branch {
    label: Option<SharedString>,
    stop: Option<CancellationToken>,
    task: Option<Task<()>>,
}

impl Drop for Branch {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop {
            stop.cancel();
        }
    }
}

impl Shell {
    pub(super) fn load_sidebar_branch(&mut self, id: SessionId, cx: &mut Context<Self>) {
        let Some(live) = &self.live else { return };
        let Some(session) = live
            .view
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
        else {
            return;
        };
        let scope = (live.selected, session.worktree);
        if self
            .sidebar
            .branches
            .get(&scope)
            .is_some_and(|branch| branch.task.is_some())
        {
            return;
        }
        // The controller Client shares in-flight Git reads with conversations and packages.
        let client = live.client();
        let stop = CancellationToken::new();
        let current = stop.clone();
        let job = cx
            .global::<crate::backend::Services>()
            .runtime
            .spawn(async move {
                tokio::select! {
                    _ = stop.cancelled() => None,
                    result = client.execute(client.prepare(Command::InspectGit { worktree: scope.1 })) => Some(result),
                }
            });
        let branch = self.sidebar.branches.entry(scope).or_default();
        branch.stop = Some(current.clone());
        branch.task = Some(cx.spawn(async move |shell, cx| {
            let result = job.await;
            let _ = shell.update(cx, |shell, cx| {
                let Some(branch) = shell.sidebar.branches.get_mut(&scope) else {
                    return;
                };
                branch.task = None;
                branch.stop = None;
                let Some(snapshot) = shell
                    .live
                    .as_ref()
                    .filter(|live| live.selected == scope.0)
                    .and_then(|live| live.view.snapshot.as_ref())
                    .filter(|snapshot| snapshot.node == scope.0)
                else {
                    return;
                };
                if current.is_cancelled()
                    || !snapshot
                        .sessions
                        .iter()
                        .any(|session| session.id == id && session.worktree == scope.1)
                {
                    return;
                }
                let Some(tree) = snapshot.worktrees.iter().find(|tree| tree.id == scope.1) else {
                    return;
                };
                if let Ok(Some(Ok(Output::GitStatus(status)))) = result {
                    branch.label = status
                        .branch
                        .or_else(|| status.head.map(|head| head.chars().take(8).collect()))
                        .or_else(|| {
                            std::path::Path::new(&tree.path)
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                        })
                        .map(SharedString::from);
                    cx.notify();
                }
            });
        }));
    }

    pub(crate) fn live_session_row(
        &self,
        session: &sailry_protocol::Session,
        placement: Placement<'_>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let live = self.live.as_ref().unwrap();
        let session = session.clone();
        let id = session.id;
        let active = matches!(placement, Placement::Active);
        let (prefix, row_key, entry) = match placement {
            Placement::Project(entry) => ("live-session", Row::LiveSession(id), Some(entry)),
            Placement::Recent => ("recent-session", Row::RecentSession(id), None),
            Placement::Active => ("active-session", Row::ActiveSession(id), None),
        };
        let indicators = !matches!(placement, Placement::Recent);
        let node = live.selected;
        let hovered = self.sidebar.hovered == Some(row_key);
        let preference = crate::preferences::sessions::get(node, id, cx);
        let unread = indicators && session.activity.attention.unread;
        let running = indicators
            && sailry_client::activity::lane(&session) == sailry_client::activity::Lane::Running;
        let waiting = indicators
            && sailry_client::activity::lane(&session) == sailry_client::activity::Lane::Waiting;
        let worktree = indicators
            && session.project.is_some()
            && live.view.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .worktrees
                    .iter()
                    .any(|tree| tree.id == session.worktree && !tree.main)
            });
        let title = crate::activity::title(&session);
        let branch = self
            .sidebar
            .branches
            .get(&(node, session.worktree))
            .and_then(|branch| branch.label.clone());
        let row = self
            .sidebar_row(
                SharedString::from(format!("{prefix}-{id}")),
                row_key,
                self.page == Page::Conversation
                    && self.session_scope.active == Key::Session(live.selected, id),
                cx,
            )
            .when_some(entry, |row, entry| {
                row.pl(entry.indent())
                    .child(entry.guides(format!("{prefix}-{id}"), cx))
            })
            .when(running, |row| row.text_color(cx.theme().sidebar_foreground))
            .relative()
            .on_drag(
                crate::panes::Drag {
                    target: crate::panes::Target::Session(node, id),
                    session: Some(session.clone()),
                    terminal: None,
                    project: session.project,
                    title: crate::activity::title(&session),
                }
                .payload(),
                |drag, _, _, cx| {
                    cx.new(|_| {
                        drag.value()
                            .downcast_ref::<crate::panes::Drag>()
                            .unwrap()
                            .clone()
                    })
                },
            )
            .debug_selector(move || format!("{prefix}-{id}"))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |shell, event, window, cx| {
                    shell.live_resource_menu(
                        node,
                        crate::live::menus::Target::Session(id),
                        event,
                        window,
                        cx,
                    );
                }),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .h_5()
                    .items_center()
                    .when(active || session.delegation.is_some(), |row| {
                        row.child(if session.delegation.is_some() {
                            crate::ui::identicon::agent(&id.to_string(), cx).into_any_element()
                        } else {
                            crate::workspace::appearance::project(
                                &self.session_appearance(node, &session),
                                cx,
                            )
                            .into_any_element()
                        })
                    })
                    .child(super::title::Title::new(
                        format!("{prefix}-{id}-label"),
                        title,
                        hovered,
                    ))
                    .when(indicators && preference.pinned, |row| {
                        row.child(
                            Icon::new(IconName::Star)
                                .small()
                                .text_color(cx.theme().muted_foreground),
                        )
                    })
                    .when_some(branch.filter(|_| hovered), |row, branch| {
                        row.child(super::location(format!("{prefix}-{id}-branch"), branch, cx))
                    })
                    .when(running, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .debug_selector(move || format!("{prefix}-{id}-loading"))
                                .child(crate::ui::loading::mini()),
                        )
                    })
                    .when(waiting, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .debug_selector(move || format!("{prefix}-{id}-waiting"))
                                .child(
                                    Icon::new(IconName::Info)
                                        .size_4()
                                        .text_color(cx.theme().warning),
                                ),
                        )
                    })
                    .when(unread && !running && !waiting, |row| {
                        row.child(
                            div()
                                .size_2()
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(cx.theme().primary)
                                .debug_selector(move || format!("{prefix}-{id}-unread")),
                        )
                    })
                    .when(worktree && !running && !waiting, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .debug_selector(move || format!("{prefix}-{id}-worktree"))
                                .child(
                                    Icon::new(IconName::Network)
                                        .size_4()
                                        .text_color(cx.theme().muted_foreground),
                                ),
                        )
                    }),
            );
        let row = if let Some(entry) = entry {
            self.split_destination(row, entry, cx)
        } else {
            row
        };
        row.on_click(cx.listener(move |shell, _, window, cx| {
            if active {
                shell.open_activity(
                    node,
                    sailry_client::activity::Target::Session(id),
                    window,
                    cx,
                );
            } else {
                shell.reveal_session(session.clone(), window, cx);
            }
        }))
        .into_any_element()
    }
}
