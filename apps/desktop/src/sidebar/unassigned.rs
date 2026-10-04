//! Unassigned is a navigation group, never a synthetic project.
use super::*;

impl Shell {
    pub(crate) fn unassigned_sessions(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(live) = &self.live else {
            return div().into_any_element();
        };
        let empty = !live.view.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.sessions.iter().any(|session| {
                session.project.is_none()
                    && session.config.resource.is_none()
                    && session.config.assistant.is_none()
                    && session.delegation.is_none()
                    && !session.archived
            }) || snapshot.terminals.iter().any(|info| {
                info.status != sailry_protocol::terminal::Status::Closed
                    && snapshot
                        .worktrees
                        .iter()
                        .any(|tree| Some(tree.id) == info.worktree && tree.project.is_none())
            })
        });
        let node = live.selected;
        let open = !self.sidebar.unassigned_closed.contains(&node);
        Collapsible::new()
            .open(open)
            .gap_0p5()
            .min_h_8()
            .when(!open || empty, |group| group.flex_shrink_0())
            .child(
                h_flex()
                    .id("unassigned-sessions")
                    .h_8()
                    .flex_shrink_0()
                    .px_2()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().sidebar_foreground.opacity(0.5))
                    .debug_selector(|| "unassigned-sessions".into())
                    .on_hover(cx.listener(|shell, hovered, _, cx| {
                        if *hovered {
                            shell.sidebar.hovered = Some(Row::Unassigned);
                        } else if shell.sidebar.hovered == Some(Row::Unassigned) {
                            shell.sidebar.hovered = None;
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        if !shell.sidebar.unassigned_closed.remove(&node) {
                            shell.sidebar.unassigned_closed.insert(node);
                        }
                        cx.notify();
                    }))
                    .child(div().flex_1().child(tr("sessions_unassigned")))
                    .child(
                        Button::new("unassigned-add")
                            .ghost()
                            .xsmall()
                            .icon(
                                Icon::new(IconName::Plus)
                                    .size(px(14.))
                                    .text_color(cx.theme().sidebar_foreground.opacity(0.5)),
                            )
                            .when(self.sidebar.hovered != Some(Row::Unassigned), |button| {
                                button.opacity(0.).focus(|style| style.opacity(1.))
                            })
                            .tooltip(tr("workspace_new_session"))
                            .accessibility_label(tr("workspace_new_session"))
                            .debug_selector(|| "unassigned-add".into())
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                cx.stop_propagation();
                                shell.new_unassigned_conversation(node, window, cx);
                            })),
                    )
                    .child(
                        gpui_kit::base::AccordionTrigger::new("unassigned-toggle")
                            .open(open)
                            .tab_index(0)
                            .h_flex()
                            .h_8()
                            .flex_shrink_0()
                            .justify_end()
                            .aria_label(tr("sessions_unassigned"))
                            .debug_selector(|| "unassigned-toggle".into())
                            .child(
                                div().flex().w_5().items_center().justify_center().child(
                                    Icon::new(if open {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size(px(12.))
                                    .when(self.sidebar.hovered != Some(Row::Unassigned), |icon| {
                                        icon.opacity(0.)
                                    }),
                                ),
                            )
                            .on_change({
                                let shell = cx.entity().downgrade();
                                move |open, _, _, cx| {
                                    cx.stop_propagation();
                                    let _ = shell.update(cx, |shell, cx| {
                                        if open {
                                            shell.sidebar.unassigned_closed.remove(&node);
                                        } else {
                                            shell.sidebar.unassigned_closed.insert(node);
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .content(
                v_flex()
                    .id("sidebar-unassigned")
                    .relative()
                    .h_auto()
                    .min_h_0()
                    .debug_selector(|| "sidebar-unassigned-viewport".into())
                    .child(
                        v_flex()
                            .flex_none()
                            .child(self.live_resource_rows(None, cx))
                            .when(empty, |list| {
                                list.child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(tr("project_sessions_empty")),
                                )
                            }),
                    )
                    .overflow_y_scroll()
                    .track_scroll(&self.sidebar.unassigned_scroll)
                    .vertical_scrollbar(&self.sidebar.unassigned_scroll),
            )
            .into_any_element()
    }
}
