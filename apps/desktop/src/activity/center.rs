//! The notification center composes the same Kit card surfaces as scheduled tasks.
use super::*;
use crate::{
    preview::Page,
    ui::{card_list, settings_page::SettingsPage},
};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        collapsible::Collapsible,
        tab::{Tab, TabBar},
    },
    prelude::FluentBuilder as _,
};
use sailry_client::activity::{Id, Notice};
use sailry_protocol::notification::{Cause, Details};

impl Shell {
    pub(crate) fn open_notification_center(
        &mut self,
        selected: Option<Id>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activity.open = false;
        self.activity.unread_only = false;
        if let Some(id) = selected {
            self.activity.expanded.insert(id);
            self.read_notice(id, window, cx);
        }
        self.navigate(Page::Notifications, window, cx);
    }

    pub(crate) fn notification_center(&self, cx: &mut Context<Self>) -> AnyElement {
        let unread = self
            .activity
            .inbox
            .notices()
            .iter()
            .any(|notice| !notice.read && matches!(notice.id.target, Target::Notification(_)));
        let read = self
            .activity
            .inbox
            .notices()
            .iter()
            .any(|notice| notice.read && matches!(notice.id.target, Target::Notification(_)));
        let mut notices: Vec<_> = self
            .activity
            .inbox
            .notices()
            .iter()
            .filter(|notice| {
                matches!(notice.id.target, Target::Notification(_))
                    && (!self.activity.unread_only
                        || !notice.read
                        || self.activity.expanded.contains(&notice.id))
            })
            .collect();
        notices.sort_by_key(|notice| {
            std::cmp::Reverse((notice.timestamp_ms.unwrap_or(i64::MAX), notice.id.cursor))
        });
        let rows = notices
            .into_iter()
            .map(|notice| self.center_row(notice, cx))
            .collect::<Vec<_>>();
        SettingsPage::new("notification-center", tr("notification_center"))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        TabBar::new("notification-filter")
                            .segmented()
                            .selected_index(usize::from(self.activity.unread_only))
                            .child(Tab::new().label(tr("notification_all")))
                            .child(Tab::new().label(tr("notification_unread")))
                            .on_click(cx.listener(|shell, index, _, cx| {
                                shell.activity.unread_only = *index == 1;
                                shell.activity.expanded.clear();
                                cx.notify();
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("center-read-all")
                                    .ghost()
                                    .small()
                                    .label(tr("notifications_read_all"))
                                    .disabled(!unread || self.activity.pending)
                                    .on_click(cx.listener(|shell, _, window, cx| {
                                        shell.update_notices(false, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("center-delete-read")
                                    .ghost()
                                    .small()
                                    .label(tr("notification_delete_read"))
                                    .disabled(!read || self.activity.pending)
                                    .on_click(cx.listener(|shell, _, window, cx| {
                                        shell.update_notices(true, window, cx)
                                    })),
                            ),
                    ),
            )
            .when(rows.is_empty(), |page| {
                page.child(crate::empty_state::list(
                    IconName::Bell,
                    "notifications_empty",
                    cx,
                ))
            })
            .child(card_list::list("notification-center-items".into(), rows))
            .into_any_element()
    }

    fn center_row(&self, notice: &Notice, cx: &mut Context<Self>) -> AnyElement {
        let id = notice.id;
        let key = format!("center-{id:?}");
        let expanded = self.activity.expanded.contains(&id);
        let source = self.notice_source(notice);
        let time = notice.timestamp_ms.map(presentation::timestamp);
        let title = presentation::heading(notice);
        let trigger = Button::new(format!("{key}-open"))
            .ghost()
            .w_full()
            .h_auto()
            .py_3()
            .px_0()
            .justify_start()
            .accessibility_label(title.clone())
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_3()
                    .child(notifications::glyph(notice.kind, cx).size_5())
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .items_start()
                            .gap_1()
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .when(!notice.read, |title| title.font_medium())
                                    .child(title),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(source),
                            ),
                    )
                    .when_some(time, |row, time| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(time),
                        )
                    })
                    .when(!notice.read, |row| {
                        row.child(div().size_1p5().rounded_full().bg(cx.theme().primary))
                    })
                    .child(
                        Icon::new(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size_4()
                        .text_color(cx.theme().muted_foreground),
                    ),
            )
            .on_click(cx.listener(move |shell, _, window, cx| {
                if !shell.activity.expanded.remove(&id) {
                    shell.activity.expanded.insert(id);
                    shell.read_notice(id, window, cx);
                }
                cx.notify();
            }));
        card_list::frame(
            key,
            Collapsible::new()
                .open(expanded)
                .w_full()
                .child(trigger)
                .content(self.center_details(notice, cx)),
            cx,
        )
        .into_any_element()
    }

    fn center_details(&self, notice: &Notice, cx: &mut Context<Self>) -> AnyElement {
        let mut body = v_flex().w_full().min_w_0().gap_3().pb_4().text_sm();
        if !notice.message.trim().is_empty() {
            body = body.child(notice.message.clone());
        }
        match &notice.details {
            Some(Details::Diagnostic(diagnostic)) => match &diagnostic.cause {
                Cause::Compaction(failure) => {
                    for text in presentation::failure(failure) {
                        body =
                            body.child(div().text_color(cx.theme().muted_foreground).child(text));
                    }
                }
                Cause::Execution { code } => {
                    body = body.child(tr(crate::conversation::live::failure_key(
                        &sailry_protocol::Fault::new(*code, ""),
                    )));
                }
            },
            Some(Details::Task(task)) => {
                body = body.child(detail_line(
                    "notification_result",
                    presentation::status(task.status),
                    cx,
                ));
                if let Some(duration) = task
                    .started_ms
                    .zip(task.finished_ms)
                    .map(|(start, end)| end.saturating_sub(start).max(0) / 1000)
                {
                    body = body.child(detail_line(
                        "notification_duration",
                        rust_i18n::t!("notification_seconds", count = duration)
                            .to_string()
                            .into(),
                        cx,
                    ));
                }
                if let Some(code) = task.error {
                    body = body.child(tr(crate::conversation::live::failure_key(
                        &sailry_protocol::Fault::new(code, ""),
                    )));
                }
            }
            Some(Details::Terminal(terminal)) => {
                body = body.child(detail_line(
                    "notification_result",
                    presentation::label(notice),
                    cx,
                ));
                if let Some(code) = terminal.code {
                    body = body.child(detail_line(
                        "notification_exit_code",
                        code.to_string().into(),
                        cx,
                    ));
                }
            }
            Some(Details::Power { code }) => {
                body = body
                    .child(tr("power_tasks_continue"))
                    .child(code.to_string());
            }
            Some(Details::Text { body: text }) => body = body.child(text.clone()),
            None => {}
        }
        body = body.child(detail_line(
            "notification_source",
            self.notice_source(notice),
            cx,
        ));
        if let Some(time) = notice.timestamp_ms {
            body = body.child(detail_line(
                "notification_time",
                presentation::timestamp(time),
                cx,
            ));
        }
        if let Some(session) = notice.session {
            let title = self
                .activity_snapshot(notice.id.node)
                .and_then(|snapshot| snapshot.sessions.iter().find(|entry| entry.id == session))
                .map(|entry| sessions::title(entry))
                .unwrap_or_else(|| session.to_string().into());
            body = body.child(detail_line("conversation", title, cx));
        }
        if let Some(report) = presentation::report(notice) {
            body = body.child(self.technical_details(notice.id, report, cx));
        }
        let id = notice.id;
        let available = self.notice_available(notice, cx);
        body.child(
            h_flex().gap_2().child(
                Button::new(format!("center-action-{id:?}"))
                    .outline()
                    .small()
                    .label(tr(match notice.target() {
                        Target::Session(_) => "notification_open_conversation",
                        Target::Terminal(_) => "notification_open_terminal",
                        Target::Notification(_) => "notification_open_source",
                    }))
                    .disabled(!available)
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.open_activity(id.node, id.target, window, cx)
                    })),
            ),
        )
        .into_any_element()
    }

    fn technical_details(&self, id: Id, report: String, cx: &mut Context<Self>) -> AnyElement {
        let expanded = self.activity.technical.contains(&id);
        let copied = report.clone();
        Collapsible::new()
            .w_full()
            .open(expanded)
            .child(
                Button::new(format!("center-technical-{id:?}"))
                    .ghost()
                    .small()
                    .icon(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .label(tr("notification_technical"))
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        if !shell.activity.technical.remove(&id) {
                            shell.activity.technical.insert(id);
                        }
                        cx.notify();
                    })),
            )
            .content(
                v_flex()
                    .gap_3()
                    .pt_2()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(cx.theme().muted_foreground)
                            .child(report),
                    )
                    .child(
                        Button::new(format!("center-copy-{id:?}"))
                            .outline()
                            .small()
                            .icon(IconName::Copy)
                            .label(tr("notification_copy_report"))
                            .on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                                crate::feedback::toast(
                                    window,
                                    tr("notification_report_copied"),
                                    gpui_kit::component::notification::Notification::success(tr(
                                        "notification_report_copied",
                                    )),
                                    cx,
                                );
                            }),
                    ),
            )
            .into_any_element()
    }

    fn notice_source(&self, notice: &Notice) -> SharedString {
        let host = self
            .live
            .as_ref()
            .map(|live| live.name(notice.id.node))
            .unwrap_or_default();
        let source = if notice.package.is_empty() {
            tr("notification_system")
        } else {
            notice.package.clone().into()
        };
        if host.is_empty() {
            source
        } else {
            format!("{source} · {host}").into()
        }
    }

    fn notice_available(&self, notice: &Notice, cx: &App) -> bool {
        let Some(snapshot) = self.activity_snapshot(notice.id.node) else {
            return false;
        };
        match notice.target() {
            Target::Session(id) => snapshot.sessions.iter().any(|session| session.id == id),
            Target::Terminal(id) => snapshot.terminals.iter().any(|terminal| {
                terminal.id == id && terminal.status != sailry_protocol::terminal::Status::Closed
            }),
            Target::Notification(_) => self
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.node == notice.id.node && entry.package.name == notice.package),
        }
    }
}

fn detail_line(label: &'static str, value: SharedString, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .min_w_0()
        .gap_3()
        .items_start()
        .child(
            div()
                .w_24()
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(tr(label)),
        )
        .child(div().flex_1().min_w_0().child(value))
}
