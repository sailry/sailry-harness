//! Session rows keep navigation and actions on Kit controls with rich hover details.
use super::*;
use crate::live::menus;
use crate::shell::session_scope::Key;
use gpui_kit::component::hover_card::HoverCard;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, Output, SessionId};

pub(crate) enum Placement<'a> {
    Project(&'a super::tree::Entry),
    Recent { last: bool },
    Pinned { last: bool },
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
        let pinned = matches!(placement, Placement::Pinned { .. });
        let (prefix, row_key, entry, last) = match placement {
            Placement::Project(entry) => (
                "live-session",
                Row::LiveSession(id),
                Some(entry),
                entry.last,
            ),
            Placement::Recent { last } => ("recent-session", Row::RecentSession(id), None, last),
            Placement::Pinned { last } => ("pinned-session", Row::PinnedSession(id), None, last),
        };
        let indicators = !matches!(placement, Placement::Recent { .. });
        let node = live.selected;
        let hovered = self.sidebar.hovered == Some(row_key);
        let preference = crate::preferences::pins::get(node, id, cx);
        let project = (pinned && preference.pinned && session.project.is_some())
            .then(|| self.session_project_name(node, &session));
        let unread = indicators && session.activity.attention.unread;
        let activity = self.session_lane(node, &session);
        let running = indicators && activity == sailry_client::activity::Lane::Running;
        let waiting = indicators && activity == sailry_client::activity::Lane::Waiting;
        let busy = matches!(
            activity,
            sailry_client::activity::Lane::Running | sailry_client::activity::Lane::Waiting
        );
        let age = session
            .activity
            .run
            .as_ref()
            .filter(|_| !busy)
            .and_then(|run| run.finished_ms.or(run.started_ms))
            .map(|timestamp| age(chrono::Utc::now().timestamp_millis() - timestamp));
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
        let details = super::details::Details {
            title: title.clone(),
            project: session
                .project
                .map(|_| self.session_project_name(node, &session)),
            branch,
            path: self.activity_snapshot(node).and_then(|snapshot| {
                snapshot
                    .worktrees
                    .iter()
                    .find(|tree| tree.id == session.worktree)
                    .map(|tree| tree.path.clone().into())
            }),
            host: live.name(node),
        };
        let status = if running {
            Some((
                crate::ui::loading::small()
                    .debug_selector(move || format!("{prefix}-{id}-loading"))
                    .into_any_element(),
                "activity_running",
            ))
        } else if waiting {
            Some((
                div()
                    .flex_shrink_0()
                    .debug_selector(move || format!("{prefix}-{id}-waiting"))
                    .child(
                        Icon::new(IconName::Info)
                            .xsmall()
                            .text_color(cx.theme().warning),
                    )
                    .into_any_element(),
                "activity_waiting",
            ))
        } else if unread {
            Some((
                div()
                    .size_2()
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(cx.theme().primary)
                    .debug_selector(move || format!("{prefix}-{id}-unread"))
                    .into_any_element(),
                "notification_unread",
            ))
        } else {
            None
        };
        let accessible: SharedString = status.as_ref().map_or_else(
            || title.clone(),
            |(_, key)| format!("{title} · {}", tr(key)).into(),
        );
        let status = status.map(|(element, _)| element);
        let guides = match entry {
            Some(entry) => entry.guides(format!("{prefix}-{id}"), status, cx),
            None => super::branch(format!("{prefix}-{id}"), last, px(30.), status, cx),
        };
        let row = self
            .sidebar_row(
                SharedString::from(format!("{prefix}-{id}")),
                row_key,
                self.page == Page::Conversation
                    && self.session_scope.active == Key::Session(live.selected, id),
                cx,
            )
            .accessibility_label(accessible)
            .h_7()
            .flex_shrink_0()
            .pl(entry.map_or(px(30.), |entry| entry.indent()))
            .child(guides)
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
                    .when(session.delegation.is_some(), |row| {
                        row.child(
                            crate::ui::identicon::child(
                                &id.to_string(),
                                session.activity.run.as_ref().is_some_and(|run| {
                                    matches!(
                                        run.status,
                                        sailry_protocol::conversation::Status::Running
                                            | sailry_protocol::conversation::Status::Stopping
                                    )
                                }),
                                cx,
                            )
                            .into_any_element(),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .debug_selector(move || format!("{prefix}-{id}-label"))
                            .child(title),
                    )
                    .when_some(project.filter(|_| !hovered), |row, project| {
                        row.child(
                            div()
                                .min_w_0()
                                .max_w_20()
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(project),
                        )
                    })
                    .when(worktree, |row| {
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
                    })
                    .when(hovered && session.delegation.is_none(), |row| {
                        row.child(
                            h_flex()
                                .flex_shrink_0()
                                .gap_0p5()
                                .child(
                                    Button::new(format!("{prefix}-{id}-pin"))
                                        .text()
                                        .xsmall()
                                        .size_5()
                                        .icon(if preference.pinned {
                                            gpui_kit::assets::IconName::PinOff
                                        } else {
                                            gpui_kit::assets::IconName::Pin
                                        })
                                        .accessibility_label(tr(if preference.pinned {
                                            "workspace_unpin"
                                        } else {
                                            "workspace_pin"
                                        }))
                                        .on_click(cx.listener(move |shell, _, window, cx| {
                                            cx.stop_propagation();
                                            shell.live_resource_action(
                                                &menus::Dispatch {
                                                    node,
                                                    target: menus::Target::Session(id),
                                                    command: if preference.pinned {
                                                        menus::Command::Unpin
                                                    } else {
                                                        menus::Command::Pin
                                                    },
                                                },
                                                window,
                                                cx,
                                            );
                                        })),
                                )
                                .child(self.live_session_archive(node, &session, cx)),
                        )
                    })
                    .when_some(age, |row, age| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                // Dimmer than titles so the age reads as secondary.
                                .text_color(cx.theme().muted_foreground.opacity(0.6))
                                .debug_selector(move || format!("{prefix}-{id}-age"))
                                .child(age),
                        )
                    }),
            );
        let row = if let Some(entry) = entry {
            self.split_destination(row, entry, cx)
        } else {
            row
        };
        let row = row.on_click(cx.listener(move |shell, _, window, cx| {
            if pinned && !preference.pinned {
                shell.open_activity(
                    node,
                    sailry_client::activity::Target::Session(id),
                    window,
                    cx,
                );
            } else {
                shell.reveal_session(session.clone(), window, cx);
            }
        }));
        HoverCard::new(format!("{prefix}-{id}-details"))
            .anchor(Anchor::LeftCenter)
            .trigger_style(StyleRefinement::default().w_full().flex_shrink_0())
            .trigger(row)
            .content(move |_, window, cx| details.render(window, cx))
            .into_any_element()
    }
}

fn age(elapsed_ms: i64) -> SharedString {
    let seconds = (elapsed_ms.max(0) / 1000) as u64;
    let (key, count) = match seconds {
        0..60 => ("sidebar_age_seconds", seconds),
        60..3600 => ("sidebar_age_minutes", seconds / 60),
        3600..86_400 => ("sidebar_age_hours", seconds / 3600),
        _ => ("sidebar_age_days", seconds / 86_400),
    };
    rust_i18n::t!(key, count = count).to_string().into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn duration_labels() {
        for (elapsed_ms, expected) in [
            (-1000, "0s"),
            (0, "0s"),
            (999, "0s"),
            (1000, "1s"),
            (59_999, "59s"),
            (60_000, "1m"),
            (300_000, "5m"),
            (3_599_999, "59m"),
            (3_600_000, "1h"),
            (72_000_000, "20h"),
            (86_399_999, "23h"),
            (86_400_000, "1d"),
            (604_799_999, "6d"),
            (604_800_000, "7d"),
            (950_400_000, "11d"),
            (2_592_000_000, "30d"),
        ] {
            assert_eq!(super::age(elapsed_ms).as_ref(), expected, "{elapsed_ms}");
        }
    }
}
