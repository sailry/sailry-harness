use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::{Notification, NotificationDelivery, NotificationType};
use sailry_client::activity::{Kind, Notice};
use std::time::Duration;

struct NoticeView;

impl Shell {
    /// One admission path for notices: Client owns deduplication and unread state;
    /// this module owns in-app and system presentation.
    pub(super) fn receive_notices(
        &mut self,
        node: NodeId,
        notices: &[Notice],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for notice in self.activity.inbox.notices().iter().filter(|notice| {
            notice.id.node == node
                && matches!(notice.id.target, Target::Notification(_))
                && !notices
                    .iter()
                    .any(|next| next.id == notice.id && !next.read)
        }) {
            window.remove_notification1::<NoticeView>(format!("{:?}", notice.id), cx);
        }
        for notice in self.activity.inbox.merge(node, notices) {
            self.activity
                .received
                .insert(notice.id, std::time::Instant::now());
            self.publish_notice(&notice, window, cx);
            if matches!(notice.kind, Kind::Completed | Kind::Failed)
                && let Target::Session(session) = notice.id.target
                && self.current_session_is_visible(node, session, window)
            {
                self.activity.read_session(node, session);
                self.set_session_read(node, session, true, window, cx);
            }
        }
        self.activity.received.retain(|id, _| {
            self.activity
                .inbox
                .notices()
                .iter()
                .any(|notice| notice.id == *id)
        });
    }

    fn publish_notice(&mut self, notice: &Notice, window: &mut Window, cx: &mut Context<Self>) {
        let preferences = crate::preferences::data(cx);
        let (index, label, kind) = match notice.kind {
            Kind::Notification(kind) => match kind {
                sailry_protocol::notification::Kind::Info => {
                    (1, "notifications", NotificationType::Info)
                }
                sailry_protocol::notification::Kind::Success => {
                    (2, "notifications", NotificationType::Success)
                }
                sailry_protocol::notification::Kind::Warning => {
                    (1, "notifications", NotificationType::Warning)
                }
                sailry_protocol::notification::Kind::Error => {
                    (3, "notifications", NotificationType::Error)
                }
            },
            Kind::Approval => (1, "activity_approval", NotificationType::Warning),
            Kind::Input => (1, "activity_input", NotificationType::Info),
            Kind::Completed => (2, "activity_completed", NotificationType::Success),
            Kind::Failed => (3, "activity_failed", NotificationType::Error),
        };
        if !preferences.notifications[index] {
            return;
        }
        let delivery = if preferences.notifications[0] && !window.is_window_active() {
            NotificationDelivery::System
        } else {
            NotificationDelivery::InApp
        };
        let id = notice.id;
        let key = format!("{id:?}");
        let message = if notice.title.is_empty() {
            match id.target {
                Target::Notification(_) => tr("notifications"),
                Target::Session(_) => tr("conversation"),
                Target::Terminal(_) => tr("terminal"),
            }
        } else {
            if notice.message.is_empty() {
                notice.title.clone().into()
            } else {
                format!("{}\n{}", notice.title, notice.message).into()
            }
        };
        let owner = cx.weak_entity();
        let action = match id.target {
            Target::Notification(_) => "notification_open",
            Target::Session(_) => "workspace_open_session",
            Target::Terminal(_) => "workspace_open_terminal",
        };
        window.push_notification(
            Notification::new()
                .id1::<NoticeView>(key.clone())
                .min_w(crate::feedback::minimum_width(window))
                .max_w(px(520.).min(window.viewport_size().width - px(32.)))
                .title(tr(label))
                .message(message)
                .with_type(kind)
                .delivery(delivery)
                .autohide(false)
                .action(move |_, _, _| {
                    let owner = owner.clone();
                    Button::new("notification-open")
                        .ghost()
                        .icon(IconName::ArrowRight)
                        .tooltip(tr(action))
                        .accessibility_label(tr(action))
                        .debug_selector(|| "notification-open".into())
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = owner.update(cx, |shell, cx| {
                                shell.read_notice(id, window, cx);
                                shell.open_activity(id.node, id.target, window, cx);
                            });
                            window.remove_notification1::<NoticeView>(format!("{id:?}"), cx);
                        })
                })
                .on_click(cx.listener(move |shell, _, window, cx| {
                    shell.read_notice(id, window, cx);
                    shell.open_activity(id.node, id.target, window, cx);
                })),
            cx,
        );
        if delivery.includes_in_app() {
            // Kit b79f4ce fixes autohide at five seconds. Its public dismissal
            // API applies the selected duration without replacing the toast.
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(preferences.toast_seconds))
                    .await;
                let _ = cx.update(|window, cx| window.remove_notification1::<NoticeView>(key, cx));
            })
            .detach();
        }
    }
}
