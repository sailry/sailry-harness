//! Local pins precede transient activity for the selected Host.
use super::*;
use sailry_protocol::{NodeId, Session, SessionId};
use std::cmp::Reverse;

#[cfg(test)]
mod tests;

fn compose(
    node: NodeId,
    sessions: &[Session],
    active: Vec<(NodeId, Session)>,
    preference: impl Fn(SessionId) -> crate::preferences::sessions::State,
) -> Vec<Session> {
    let mut pinned: Vec<_> = sessions
        .iter()
        .filter(|session| {
            !session.archived && session.delegation.is_none() && preference(session.id).pinned
        })
        .cloned()
        .collect();
    pinned.sort_by_key(|session| {
        let state = preference(session.id);
        let last = session
            .activity
            .run
            .as_ref()
            .and_then(|run| run.finished_ms.or(run.started_ms));
        (
            state.order.is_some(),
            state.order,
            Reverse(last),
            Reverse(session.creation_order),
        )
    });
    let ids: BTreeSet<_> = pinned.iter().map(|session| session.id).collect();
    pinned.extend(
        active
            .into_iter()
            .filter(|(owner, session)| *owner == node && !ids.contains(&session.id))
            .map(|(_, session)| session),
    );
    pinned
}

impl Shell {
    pub(super) fn pinned_sessions(&self, cx: &App) -> Vec<Session> {
        let Some(live) = &self.live else {
            return Vec::new();
        };
        let node = live.selected;
        let sessions = self
            .activity_snapshot(node)
            .filter(|snapshot| snapshot.node == node)
            .map(|snapshot| snapshot.sessions.as_slice())
            .unwrap_or_default();
        compose(node, sessions, self.active_sessions(), |id| {
            crate::preferences::sessions::get(node, id, cx)
        })
    }

    pub(super) fn pinned_group(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (rows, group, more) = if let Some(live) = &self.live {
            let sessions = self.pinned_sessions(cx);
            let group = limits::Group::Pinned(live.selected);
            let more = sessions.len() > limits::INITIAL;
            let count = sessions.len().min(self.sidebar.session_limit(group));
            let rows: Vec<_> = sessions
                .into_iter()
                .take(count)
                .enumerate()
                .map(|(index, session)| {
                    self.live_session_row(
                        &session,
                        sessions::Placement::Pinned {
                            last: index + 1 == count,
                        },
                        cx,
                    )
                })
                .collect();
            (rows, group, more)
        } else {
            let group = limits::Group::PreviewPinned(self.host);
            let mut sessions: Vec<_> = self
                .workspace
                .sessions
                .iter()
                .filter(|(key, session)| key.0 == self.host && session.pinned && !session.archived)
                .collect();
            sessions.sort_by_key(|(key, _)| {
                self.sidebar
                    .preview_pins
                    .iter()
                    .position(|pinned| pinned == *key)
            });
            let more = sessions.len() > limits::INITIAL;
            sessions.truncate(self.sidebar.session_limit(group));
            let count = sessions.len();
            let rows = sessions
                .into_iter()
                .enumerate()
                .map(|(index, (&key, session))| {
                    self.sidebar_row(
                        ("pinned-preview", key.1),
                        Row::Session(key.0, key.1),
                        self.page == Page::Conversation && (self.host, self.session) == key,
                        cx,
                    )
                    .pl(px(30.))
                    .child(super::branch(
                        format!("pinned-preview-{}", key.1),
                        index + 1 == count,
                        px(30.),
                        None,
                        cx,
                    ))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |shell, event, window, cx| {
                            shell.workspace_menu(
                                crate::workspace::Target::Session(key),
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
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(session.title.clone()),
                            )
                            .child(
                                div()
                                    .max_w_20()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        self.workspace.projects[&session.owner.project]
                                            .name
                                            .clone(),
                                    ),
                            ),
                    )
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.select_session(key, window, cx)
                    }))
                    .into_any_element()
                })
                .collect();
            (rows, group, more)
        };
        if rows.is_empty() {
            return None;
        }
        let open = !self.sidebar.pinned_closed;
        Some(
            Collapsible::new()
                .open(open)
                .flex_shrink_0()
                .child(
                    gpui_kit::base::AccordionTrigger::new("pinned-toggle")
                        .open(open)
                        .tab_index(0)
                        .h_flex()
                        .h_8()
                        .flex_shrink_0()
                        .px_2()
                        .gap_1()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground.opacity(0.7))
                        .aria_label(tr("sidebar_pinned"))
                        .debug_selector(|| "pinned-toggle".into())
                        .on_hover(cx.listener(|shell, hovered, _, cx| {
                            if *hovered {
                                shell.sidebar.hovered = Some(Row::Pinned);
                            } else if shell.sidebar.hovered == Some(Row::Pinned) {
                                shell.sidebar.hovered = None;
                            }
                            cx.notify();
                        }))
                        .child(div().flex_1().child(tr("sidebar_pinned")))
                        .child(
                            Icon::new(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(px(12.))
                            .when(self.sidebar.hovered != Some(Row::Pinned), |icon| {
                                icon.opacity(0.)
                            }),
                        )
                        .on_change({
                            let shell = cx.entity().downgrade();
                            move |open, _, _, cx| {
                                cx.stop_propagation();
                                let _ = shell.update(cx, |shell, cx| {
                                    shell.sidebar.pinned_closed = !open;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .content(
                    v_flex()
                        .id("sidebar-pinned")
                        .relative()
                        .h_auto()
                        .max_h(rems(7.375))
                        .min_h_0()
                        .debug_selector(|| "sidebar-pinned".into())
                        .overflow_y_scroll()
                        .track_scroll(&self.sidebar.pinned_scroll)
                        .child(
                            v_flex()
                                .flex_none()
                                .gap_0p5()
                                .children(rows)
                                .children(self.more_sessions(group, more, cx)),
                        ),
                )
                .into_any_element(),
        )
    }
}
