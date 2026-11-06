//! Session rows keep inline location metadata, navigation and menus on the Kit ListItem.
//! Hover metadata restores f4d8317d's layout, removed in 3db2cc5f, alongside current status rows.
use super::*;
use crate::shell::session_scope::Key;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, Output, SessionId};

pub(crate) enum Placement<'a> {
    Project(&'a super::tree::Entry),
    Recent { last: bool },
    Active { last: bool },
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
        let active = matches!(placement, Placement::Active { .. });
        let (prefix, row_key, entry, branch_last) = match placement {
            Placement::Project(entry) => ("live-session", Row::LiveSession(id), Some(entry), None),
            Placement::Recent { last } => {
                ("recent-session", Row::RecentSession(id), None, Some(last))
            }
            Placement::Active { last } => {
                ("active-session", Row::ActiveSession(id), None, Some(last))
            }
        };
        let indicators = !matches!(placement, Placement::Recent { .. });
        let node = live.selected;
        let hovered = self.sidebar.hovered == Some(row_key);
        let preference = crate::preferences::sessions::get(node, id, cx);
        let unread = indicators && session.activity.attention.unread;
        let activity = self.session_lane(node, &session);
        let running = indicators && activity == sailry_client::activity::Lane::Running;
        let waiting = indicators && activity == sailry_client::activity::Lane::Waiting;
        // Idle rows show when they last ran; hover swaps in the branch and busy rows
        // keep only the status on their branch elbow.
        let busy = matches!(
            activity,
            sailry_client::activity::Lane::Running | sailry_client::activity::Lane::Waiting
        );
        let age = session
            .activity
            .run
            .as_ref()
            .filter(|_| !busy && !hovered)
            .and_then(|run| run.finished_ms.or(run.started_ms))
            .map(|timestamp| age(chrono::Utc::now().timestamp_millis() - timestamp, timestamp));
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
        let status = if running {
            Some((
                crate::ui::loading::mini()
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
        let row = self
            .sidebar_row(
                SharedString::from(format!("{prefix}-{id}")),
                row_key,
                self.page == Page::Conversation
                    && self.session_scope.active == Key::Session(live.selected, id),
                cx,
            )
            .accessibility_label(accessible)
            .map(|row| match (entry, branch_last) {
                (Some(entry), _) => {
                    row.pl(entry.indent())
                        .child(entry.guides(format!("{prefix}-{id}"), status, cx))
                }
                (None, Some(last)) => row.pl(px(30.)).child(super::branch(
                    format!("{prefix}-{id}"),
                    last,
                    px(30.),
                    status,
                    cx,
                )),
                (None, None) => row,
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

/// Relative age within a week, then the local date of the last run.
fn age(elapsed_ms: i64, timestamp_ms: i64) -> SharedString {
    let seconds = (elapsed_ms.max(0) / 1000) as u64;
    if seconds < 60 {
        return tr("notifications_time_now");
    }
    let (key, count) = match seconds {
        60..3600 => ("notifications_time_minutes", seconds / 60),
        3600..86_400 => ("notifications_time_hours", seconds / 3600),
        86_400..604_800 => ("notifications_time_days", seconds / 86_400),
        _ => {
            use chrono::{Datelike, TimeZone};
            let Some(date) = chrono::Local.timestamp_millis_opt(timestamp_ms).single() else {
                return SharedString::default();
            };
            let format = if date.year() == chrono::Local::now().year() {
                "%m-%d"
            } else {
                "%Y-%m-%d"
            };
            return date.format(format).to_string().into();
        }
    };
    rust_i18n::t!(key, count = count).to_string().into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn duration_labels() {
        for (scenario, check) in [
            (
                "formats_unit_boundaries",
                (|_scenario: &str| {
                    let now = chrono::Utc::now().timestamp_millis();
                    for elapsed_ms in [-1000, 0, 59_999] {
                        assert_eq!(
                            super::age(elapsed_ms, now).as_ref(),
                            crate::tr("notifications_time_now").as_ref(),
                            "formats unit boundaries"
                        );
                    }
                    for (elapsed_ms, key, count) in [
                        (60_000, "notifications_time_minutes", 1u64),
                        (3_599_999, "notifications_time_minutes", 59),
                        (3_600_000, "notifications_time_hours", 1),
                        (86_399_999, "notifications_time_hours", 23),
                        (86_400_000, "notifications_time_days", 1),
                        (604_799_999, "notifications_time_days", 6),
                    ] {
                        assert_eq!(
                            super::age(elapsed_ms, now).as_ref(),
                            rust_i18n::t!(key, count = count).as_ref(),
                            "formats unit boundaries: elapsed_ms={elapsed_ms}"
                        );
                    }
                    let date = super::age(604_800_000, now - 604_800_000);
                    assert!(!date.is_empty(), "formats unit boundaries");
                    assert!(
                        date.chars()
                            .all(|char| char.is_ascii_digit() || char == '-'),
                        "formats unit boundaries: {date}"
                    );
                }) as fn(&str),
            ),
            (
                "formats_recent_age",
                (|_scenario: &str| {
                    let now = chrono::Utc::now().timestamp_millis();
                    assert_eq!(
                        super::age(30_000, now).as_ref(),
                        crate::tr("notifications_time_now").as_ref(),
                        "formats recent age"
                    );
                    assert_eq!(
                        super::age(5 * 60_000, now).as_ref(),
                        rust_i18n::t!("notifications_time_minutes", count = 5u64).as_ref(),
                        "formats recent age"
                    );
                    let old = super::age(30 * 86_400_000, now - 30 * 86_400_000);
                    assert!(
                        old.chars().all(|char| char.is_ascii_digit() || char == '-'),
                        "formats recent age: {old}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
