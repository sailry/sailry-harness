//! Backup selection is scoped to the restore action, not the settings page.
use super::*;
use crate::{
    settings::{entry::Entry, group::Group},
    theme::DialogStyle as _,
    tr,
};
use gpui_kit::{
    component::{button::Button, scroll::ScrollableElement},
    prelude::FluentBuilder as _,
};

struct Records {
    owner: Entity<Panel>,
}
impl Panel {
    pub(super) fn open_records(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        self.records_open = true;
        self.page = None;
        self.list(window, cx);
        let panel = cx.entity();
        let owner = panel.clone();
        let records = cx.new(|cx| {
            cx.observe_in(&panel, window, |_, _, _, cx| cx.notify())
                .detach();
            Records { owner: panel }
        });
        window.open_dialog(cx, move |dialog, window, cx| {
            let close = owner.clone();
            dialog
                .form_title(tr("backup_restore_storage"), cx)
                .w((window.viewport_size().width - px(48.)).min(px(640.)))
                .on_close(move |_, _, cx| {
                    close.update(cx, |panel, _| {
                        panel.records_open = false;
                        panel.page = None;
                    })
                })
                .child(records.clone())
        });
    }
}
impl Render for Records {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = self.owner.read(cx);
        let mut rows = Group::new("backup_records")
            .heading(false)
            .card(false)
            .empty(IconName::Inbox, "backup_empty");
        if let Some(page) = &panel.page {
            for record in &page.records {
                let label = chrono::DateTime::from_timestamp_millis(
                    record.created_ms.min(i64::MAX as u64) as i64,
                )
                .map(|time| {
                    time.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| record.id.clone());
                let mut row = Entry::new(
                    format!("backup-record-{}", record.id),
                    v_flex().gap_1().child(label).child(
                        div().text_color(cx.theme().muted_foreground).child(format!(
                            "{:.1} MiB",
                            record.size_bytes as f64 / (1024. * 1024.)
                        )),
                    ),
                );
                let copies = page.locations.get(&record.id);
                for (location, available, key) in [
                    (
                        sailry_protocol::backup::Location::Local,
                        copies.is_some_and(|copies| copies.local),
                        "backup_local",
                    ),
                    (
                        sailry_protocol::backup::Location::Online,
                        copies.is_none_or(|copies| copies.online),
                        "backup_online_enabled",
                    ),
                ] {
                    if available {
                        let owner = self.owner.clone();
                        let record = record.clone();
                        row = row.action(
                            Button::new(SharedString::from(format!(
                                "backup-restore-{}-{location:?}",
                                record.id
                            )))
                            .label(tr(key))
                            .disabled(panel.locked())
                            .on_click(move |_, window, cx| {
                                restore::open(owner.clone(), record.clone(), location, window, cx)
                            }),
                        );
                    }
                }
                rows = rows.child(row);
            }
        }
        v_flex()
            .gap_3()
            .child(rows)
            .when(panel.loading, |view| view.child(spinner::Spinner::new()))
            .when(
                panel.page.as_ref().is_some_and(|page| page.partial),
                |view| {
                    view.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("backup_partial")),
                    )
                },
            )
            .max_h((window.viewport_size().height - px(240.)).max(px(180.)))
            .overflow_y_scrollbar()
    }
}
