use super::*;
use gpui_kit::component::{
    badge::Badge,
    button::{Button, ButtonVariants},
    popover::Popover,
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_client::activity::{Kind, Notice};
use std::time::Duration;

impl Shell {
    pub(crate) fn notifications(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let unread = self.activity.inbox.unread();
        let empty = self.activity.inbox.notices().is_empty();
        let read = self
            .activity
            .inbox
            .notices()
            .iter()
            .any(|notice| notice.read && matches!(notice.id.target, Target::Notification(_)));
        let mut items = Vec::new();
        for (index, notice) in self.activity.inbox.notices().iter().enumerate() {
            let received = self
                .activity
                .received
                .get(&notice.id)
                .copied()
                .unwrap_or_else(std::time::Instant::now);
            let age = notice
                .timestamp_ms
                .map(|timestamp| {
                    elapsed(Duration::from_millis(
                        chrono::Utc::now()
                            .timestamp_millis()
                            .saturating_sub(timestamp)
                            .max(0) as u64,
                    ))
                })
                .or_else(|| {
                    self.activity
                        .received
                        .get(&notice.id)
                        .map(|received| elapsed(received.elapsed()))
                });
            items.push((received, self.notice(index, notice, age, cx)));
        }
        items.sort_by_key(|(received, _)| std::cmp::Reverse(*received));
        Popover::new("notifications")
            .anchor(Anchor::LeftCenter)
            .open(self.activity.open)
            .on_open_change(cx.listener(|shell, open, _, cx| {
                shell.activity.open = *open;
                cx.notify();
            }))
            .w(px(320.).min(window.viewport_size().width - px(32.)))
            .p_0()
            .rounded(cx.theme().radius_lg)
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
                    .id("notifications-panel")
                    .test_support()
                    .w_full()
                    .debug_selector(|| "notifications-panel".into())
                    .p_2()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .w_full()
                            .h_10()
                            .flex_shrink_0()
                            .px_2()
                            .gap_1()
                            .child(
                                h_flex().flex_1().min_w_0().child(
                                    Button::new("notification-center-open")
                                        .ghost()
                                        .small()
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .items_center()
                                                .child(tr("notification_center"))
                                                .child(Icon::new(IconName::ArrowRight).size_3()),
                                        )
                                        .accessibility_label(tr("notification_center"))
                                        .on_click(cx.listener(|shell, _, window, cx| {
                                            shell.open_notification_center(None, window, cx)
                                        })),
                                ),
                            )
                            .child(
                                Button::new("notifications-read-all")
                                    .debug_selector(|| "notifications-read-all".into())
                                    .ghost()
                                    .small()
                                    .icon(gpui_kit::assets::IconName::CheckCheck)
                                    .tooltip(tr("notifications_read_all"))
                                    .accessibility_label(tr("notifications_read_all"))
                                    .disabled(unread == 0)
                                    .on_click(cx.listener(|shell, _, window, cx| {
                                        let ids: Vec<_> = shell
                                            .activity
                                            .inbox
                                            .notices()
                                            .iter()
                                            .filter(|notice| !notice.read)
                                            .map(|notice| notice.id)
                                            .collect();
                                        for id in ids {
                                            shell.read_notice(id, window, cx);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("notifications-clear")
                                    .debug_selector(|| "notifications-clear".into())
                                    .ghost()
                                    .small()
                                    .icon(gpui_kit::assets::IconName::Trash)
                                    .tooltip(tr("notification_delete_read"))
                                    .accessibility_label(tr("notification_delete_read"))
                                    .disabled(!read || self.activity.pending)
                                    .on_click(cx.listener(|shell, _, window, cx| {
                                        shell.update_notices(true, window, cx);
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

    fn notice(
        &self,
        index: usize,
        notice: &Notice,
        age: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = notice.id;
        let label = match notice.kind {
            Kind::Notification(_) => "notifications",
            Kind::Completed => "activity_completed",
            Kind::Failed => "activity_failed",
            Kind::Approval => "activity_approval",
            Kind::Input => "activity_input",
        };
        let title = presentation::heading(notice);
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
        let mut parts = vec![title.to_string(), host.to_string(), tr(label).to_string()];
        if notice.error.is_none() && !notice.message.is_empty() {
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
            glyph(notice.kind, cx),
            age,
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
fn entry(
    id: String,
    text: String,
    detail: String,
    icon: Icon,
    age: Option<SharedString>,
    read: bool,
    cx: &App,
) -> Button {
    let selector = id.clone();
    let label_selector = format!("{id}-label");
    let icon_selector = format!("{id}-icon");
    let time_selector = format!("{id}-time");
    let unread_selector = format!("{id}-unread");
    let marker_selector = format!("{id}-marker");
    let label = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Button::new(id)
        .ghost()
        .justify_start()
        .debug_selector(move || selector.clone())
        .accessibility_label(detail)
        .tooltip(text)
        .w_full()
        .min_w_0()
        .h_10()
        .flex_shrink_0()
        .px_2()
        .py_0()
        .text_sm()
        .rounded(cx.theme().radius)
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
                .relative()
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
                        .when(!read, |label| label.font_medium())
                        .debug_selector(move || label_selector.clone())
                        .child(label),
                )
                .when_some(age, |row, age| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .debug_selector(move || time_selector.clone())
                            .child(age),
                    )
                })
                .child(
                    div()
                        .size_1p5()
                        .absolute()
                        .right(-rems(0.875))
                        .top(relative(0.5))
                        .mt(-rems(0.1875))
                        .debug_selector(move || marker_selector.clone())
                        .child(div().size_full().rounded_full().when(!read, |marker| {
                            marker
                                .bg(cx.theme().primary)
                                .debug_selector(move || unread_selector.clone())
                        })),
                ),
        )
}

pub(super) fn glyph(kind: Kind, cx: &App) -> Icon {
    use gpui_kit::assets::IconName;
    use sailry_protocol::notification::Kind as NotificationKind;
    let (icon, color) = match kind {
        Kind::Completed | Kind::Notification(NotificationKind::Success) => {
            (IconName::CircleCheck, cx.theme().success)
        }
        Kind::Failed | Kind::Notification(NotificationKind::Error) => {
            (IconName::CircleX, cx.theme().danger)
        }
        Kind::Approval | Kind::Notification(NotificationKind::Warning) => {
            (IconName::ShieldAlert, cx.theme().warning)
        }
        Kind::Input => (IconName::MessageCircleQuestionMark, cx.theme().warning),
        Kind::Notification(NotificationKind::Info) => (IconName::Info, cx.theme().muted_foreground),
    };
    Icon::new(icon).text_color(color)
}

fn elapsed(age: Duration) -> SharedString {
    let seconds = age.as_secs();
    if seconds < 60 {
        return tr("notifications_time_now");
    }
    let (key, count) = if seconds < 3600 {
        ("notifications_time_minutes", seconds / 60)
    } else if seconds < 86400 {
        ("notifications_time_hours", seconds / 3600)
    } else {
        ("notifications_time_days", seconds / 86400)
    };
    rust_i18n::t!(key, count = count).to_string().into()
}
