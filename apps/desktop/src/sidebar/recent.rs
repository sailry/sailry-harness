//! Recent conversations share the resource rows and keep their original ownership.
use super::*;

impl Shell {
    pub(super) fn recent_sessions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<AnyElement> = if let Some(live) = &self.live {
            let sessions = live
                .view
                .snapshot
                .as_ref()
                .map(sailry_client::sessions::recent)
                .unwrap_or_default();
            let count = sessions.len();
            sessions
                .into_iter()
                .enumerate()
                .map(|(index, session)| {
                    self.live_session_row(
                        session,
                        sessions::Placement::Recent {
                            last: index + 1 == count,
                        },
                        cx,
                    )
                })
                .collect()
        } else {
            let mut sessions: Vec<_> = self
                .workspace
                .sessions
                .iter()
                .filter(|(key, session)| {
                    key.0 == self.host
                        && !session.archived
                        && self.workspace.contains(session.owner)
                })
                .map(|(key, session)| (*key, session))
                .collect();
            sessions.sort_by_key(|(key, _)| std::cmp::Reverse(key.1));
            let count = sessions.len();
            sessions
                .into_iter()
                .enumerate()
                .map(|(index, (key, session))| {
                    let title = session.title.clone();
                    let branch = self.workspace.branch_label(session.owner);
                    let hovered = self.sidebar.hovered == Some(Row::RecentPreview(key.0, key.1));
                    self.sidebar_row(
                        ("recent-preview", key.1),
                        Row::RecentPreview(key.0, key.1),
                        self.page == Page::Conversation && (self.host, self.session) == key,
                        cx,
                    )
                    .pl(px(30.))
                    .relative()
                    .child(super::branch(
                        format!("recent-preview-{}", key.1),
                        index + 1 == count,
                        px(30.),
                        cx,
                    ))
                    .debug_selector(move || format!("recent-preview-{}", key.1))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(div().flex_1().min_w_0().truncate().child(title))
                            .when(hovered, |row| {
                                row.child(super::location(
                                    format!("recent-preview-{}-branch", key.1),
                                    branch,
                                    cx,
                                ))
                            }),
                    )
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.select_session(key, window, cx)
                    }))
                    .into_any_element()
                })
                .collect()
        };
        let empty = rows.is_empty();
        let height = px((32 + rows.len().max(1) * 30).min(220) as f32);
        let open = self.sidebar.recent_open;
        Collapsible::new()
            .open(open)
            .min_h_8()
            .when(open, |group| group.max_h(height))
            .when(!open, |group| group.flex_shrink_0())
            .child(
                gpui_kit::base::AccordionTrigger::new("recent-toggle")
                    .open(open)
                    .tab_index(0)
                    .h_flex()
                    .h_8()
                    .flex_shrink_0()
                    .px_2()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().sidebar_foreground.opacity(0.5))
                    .aria_label(tr("recent_sessions"))
                    .debug_selector(|| "recent-toggle".into())
                    .on_hover(cx.listener(|shell, hovered, _, cx| {
                        if *hovered {
                            shell.sidebar.hovered = Some(Row::Recent);
                        } else if shell.sidebar.hovered == Some(Row::Recent) {
                            shell.sidebar.hovered = None;
                        }
                        cx.notify();
                    }))
                    .child(div().flex_1().child(tr("recent_sessions")))
                    .child(
                        Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(px(12.))
                        .when(self.sidebar.hovered != Some(Row::Recent), |icon| {
                            icon.opacity(0.)
                        }),
                    )
                    .on_change({
                        let shell = cx.entity().downgrade();
                        move |open, _, _, cx| {
                            cx.stop_propagation();
                            let _ = shell.update(cx, |shell, cx| {
                                shell.sidebar.recent_open = open;
                                cx.notify();
                            });
                        }
                    }),
            )
            .content(
                v_flex()
                    .id("sidebar-recent")
                    .relative()
                    .h_auto()
                    .min_h_0()
                    .debug_selector(|| "sidebar-recent-viewport".into())
                    .child(
                        v_flex()
                            .flex_none()
                            .gap_0p5()
                            .children(rows)
                            .when(empty, |list| {
                                list.child(super::branch_empty(
                                    "recent-sessions-empty",
                                    tr("project_sessions_empty"),
                                    cx,
                                ))
                            }),
                    )
                    .overflow_y_scroll()
                    .track_scroll(&self.sidebar.recent_scroll)
                    .vertical_scrollbar(&self.sidebar.recent_scroll),
            )
    }
}
