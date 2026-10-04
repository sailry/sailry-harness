use super::*;
use gpui_kit::component::{
    badge::Badge,
    button::{Button, ButtonVariants},
    popover::Popover,
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_client::activity::{Kind, Lane, Notice};

impl Shell {
    pub(crate) fn notifications(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let unread = self.activity.inbox.unread();
        let empty = self.activity.inbox.notices().is_empty();
        let mut items = Vec::new();
        for (index, notice) in self.activity.inbox.notices().iter().enumerate() {
            let received = self
                .activity
                .received
                .get(&notice.id)
                .copied()
                .unwrap_or_else(std::time::Instant::now);
            items.push((received, self.notice(index, notice, cx)));
        }
        items.sort_by_key(|(received, _)| std::cmp::Reverse(*received));
        Popover::new("notifications")
            .anchor(Anchor::BottomLeft)
            .anchor_offset(Self::rail_popover_offset(window))
            .open(self.activity.open)
            .on_open_change(cx.listener(|shell, open, _, cx| {
                shell.activity.open = *open;
                cx.notify();
            }))
            .w(px(320.).min(window.viewport_size().width - px(32.)))
            .p_0()
            .trigger(
                Button::new("notifications-open")
                    .debug_selector(|| "notifications-open".into())
                    .ghost()
                    .size_9()
                    .child(
                        div()
                            .when(unread > 0, |view| {
                                view.debug_selector(|| "notifications-unread".into()).child(
                                    Badge::new()
                                        .dot()
                                        .color(cx.theme().danger)
                                        .child(Icon::new(IconName::Bell)),
                                )
                            })
                            .when(unread == 0, |view| view.child(Icon::new(IconName::Bell))),
                    )
                    .tooltip(tr("notifications"))
                    .accessibility_label(tr("notifications")),
            )
            .child(
                v_flex()
                    .w_full()
                    .debug_selector(|| "notifications-panel".into())
                    .p_2()
                    .gap_1()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .w_full()
                            .h_10()
                            .flex_shrink_0()
                            .px_2()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .font_semibold()
                                    .child(tr("notifications")),
                            )
                            .child(
                                Button::new("notifications-clear")
                                    .debug_selector(|| "notifications-clear".into())
                                    .ghost()
                                    .small()
                                    .icon(IconName::CircleX)
                                    .tooltip(tr("notifications_clear"))
                                    .accessibility_label(tr("notifications_clear"))
                                    .disabled(empty)
                                    .on_click(cx.listener(|shell, _, window, cx| {
                                        shell.clear_notices(window, cx);
                                        shell.activity.received.retain(|id, _| {
                                            shell
                                                .activity
                                                .inbox
                                                .notices()
                                                .iter()
                                                .any(|notice| notice.id == *id)
                                        });
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("notification-items")
                            .w_full()
                            .gap_1()
                            .h_auto()
                            .max_h((window.viewport_size().height - px(160.)).min(px(360.)))
                            .overflow_y_scrollbar()
                            .when(empty, |list| {
                                list.child(crate::empty_state::list(
                                    IconName::Bell,
                                    "notifications_empty",
                                    cx,
                                ))
                            })
                            .children(items.into_iter().map(|(_, item)| item)),
                    ),
            )
    }

    fn notice(&self, index: usize, notice: &Notice, cx: &mut Context<Self>) -> AnyElement {
        let id = notice.id;
        let label = match notice.kind {
            Kind::Notification(_) => "notifications",
            Kind::Completed => "activity_completed",
            Kind::Failed => "activity_failed",
            Kind::Approval => "activity_approval",
            Kind::Input => "activity_input",
        };
        let title = if notice.title.is_empty() {
            match id.target {
                Target::Notification(_) => tr("notifications"),
                Target::Session(_) => tr("conversation"),
                Target::Terminal(_) => tr("terminal"),
            }
        } else {
            notice.title.clone().into()
        };
        let host = self
            .live
            .as_ref()
            .map(|live| live.name(id.node))
            .unwrap_or_default();
        let available = self
            .activity
            .observers
            .get(&id.node)
            .and_then(|observer| observer.view.snapshot.as_ref())
            .is_some_and(|snapshot| {
                if let Target::Notification(id) = id.target {
                    return snapshot.notifications.iter().any(|notice| notice.id == id);
                }
                snapshot
                    .worktrees
                    .iter()
                    .any(|tree| Some(tree.id) == notice.worktree)
                    && match id.target {
                        Target::Session(id) => snapshot.sessions.iter().any(|s| s.id == id),
                        Target::Terminal(id) => snapshot.terminals.iter().any(|t| {
                            t.id == id && t.status != sailry_protocol::terminal::Status::Closed
                        }),
                        Target::Notification(_) => false,
                    }
            });
        let state = match notice.kind {
            Kind::Notification(kind) => match kind {
                sailry_protocol::notification::Kind::Success => Lane::Completed,
                sailry_protocol::notification::Kind::Error => Lane::Failed,
                sailry_protocol::notification::Kind::Warning => Lane::Waiting,
                sailry_protocol::notification::Kind::Info => Lane::Idle,
            },
            Kind::Completed => Lane::Completed,
            Kind::Failed => Lane::Failed,
            Kind::Approval | Kind::Input => Lane::Waiting,
        };
        let mut parts = vec![title.to_string(), host.to_string(), tr(label).to_string()];
        if !notice.message.is_empty() {
            parts.push(notice.message.clone());
        }
        if !available {
            parts.push(tr("notifications_unavailable").to_string());
        }
        let detail = parts
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        entry(
            format!("notification-{index}"),
            title.to_string(),
            detail,
            indicator(state, cx),
            notice.read,
            cx,
        )
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.read_notice(id, window, cx);
            if available {
                shell.open_activity(id.node, id.target, window, cx);
            }
            cx.notify();
        }))
        .into_any_element()
    }
}

/// Session and persistent workflow notices use the same compact, accessible row.
fn entry(id: String, text: String, detail: String, icon: Icon, read: bool, cx: &App) -> Button {
    let selector = id.clone();
    let label_selector = format!("{id}-label");
    let icon_selector = format!("{id}-icon");
    let label = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Button::new(id)
        .ghost()
        .justify_start()
        .debug_selector(move || selector.clone())
        .accessibility_label(detail.clone())
        .tooltip(detail)
        .w_full()
        .min_w_0()
        .h_9()
        .flex_shrink_0()
        .px_2()
        .py_0()
        .text_sm()
        .text_color(if read {
            cx.theme().muted_foreground
        } else {
            cx.theme().foreground
        })
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .flex_shrink_0()
                        .debug_selector(move || icon_selector.clone())
                        .child(icon.size_4()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .debug_selector(move || label_selector.clone())
                        .child(label),
                ),
        )
}
