//! Active sessions sit above projects; unread completion shares the project dot.
use super::*;

impl Shell {
    pub(super) fn active_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<_> = self
            .active_sessions()
            .into_iter()
            .filter(|(node, _)| {
                self.live
                    .as_ref()
                    .is_some_and(|live| live.selected == *node)
            })
            .map(|(_, session)| self.live_session_row(&session, sessions::Placement::Active, cx))
            .collect();
        let open = !self.sidebar.active_closed;
        let empty = rows.is_empty();
        Collapsible::new()
            .open(open)
            .flex_shrink_0()
            .max_h(px(192.))
            .child(
                gpui_kit::base::AccordionTrigger::new("active-toggle")
                    .open(open)
                    .tab_index(0)
                    .h_flex()
                    .h_8()
                    .flex_shrink_0()
                    .px_2()
                    .text_sm()
                    .text_color(cx.theme().sidebar_foreground.opacity(0.5))
                    .aria_label(tr("active_sessions"))
                    .debug_selector(|| "active-toggle".into())
                    .on_hover(cx.listener(|shell, hovered, _, cx| {
                        if *hovered {
                            shell.sidebar.hovered = Some(Row::Active);
                        } else if shell.sidebar.hovered == Some(Row::Active) {
                            shell.sidebar.hovered = None;
                        }
                        cx.notify();
                    }))
                    .child(div().flex_1().child(tr("active_sessions")))
                    .child(
                        Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(px(12.))
                        .when(self.sidebar.hovered != Some(Row::Active), |icon| {
                            icon.opacity(0.)
                        }),
                    )
                    .on_change({
                        let shell = cx.entity().downgrade();
                        move |open, _, _, cx| {
                            cx.stop_propagation();
                            let _ = shell.update(cx, |shell, cx| {
                                shell.sidebar.active_closed = !open;
                                cx.notify();
                            });
                        }
                    }),
            )
            .content(
                v_flex()
                    .id("sidebar-active")
                    .relative()
                    .h_auto()
                    .max_h(px(160.))
                    .min_h_0()
                    .debug_selector(|| "sidebar-active".into())
                    .overflow_y_scroll()
                    .track_scroll(&self.sidebar.active_scroll)
                    .vertical_scrollbar(&self.sidebar.active_scroll)
                    .child(
                        v_flex()
                            .flex_none()
                            .gap_0p5()
                            .children(rows)
                            .when(empty, |list| {
                                list.child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(tr("active_sessions_empty")),
                                )
                            }),
                    ),
            )
    }
}
