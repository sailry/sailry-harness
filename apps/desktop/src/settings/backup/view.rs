use super::*;
use crate::{
    settings::{entry::Entry, group::Group},
    tr,
};
use gpui_kit::{component::button::Button, prelude::FluentBuilder as _};

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.locked();
        let destination = self
            .settings
            .as_ref()
            .and_then(|settings| settings.destination.as_ref());
        let label = if self.settings.is_none() {
            div()
                .debug_selector(|| "backup-destination-loading".into())
                .w_64()
                .h_5()
                .into_any_element()
        } else {
            div()
                .debug_selector(|| "backup-destination-label".into())
                .child(
                    destination
                        .map(|value| format!("{} · {}", value.endpoint, value.root))
                        .unwrap_or_else(|| tr("backup_not_configured").to_string()),
                )
                .into_any_element()
        };
        let configure = cx.entity();
        let configuration = Group::new("backup_destination").heading(false).child(
            Entry::new("backup-destination-row", label).action(
                Button::new("backup-configure")
                    .debug_selector(|| "backup-configure".into())
                    .icon(IconName::Settings2)
                    .accessibility_label(tr("backup_configure"))
                    .disabled(locked)
                    .on_click(move |_, window, cx| editor::open(configure.clone(), window, cx)),
            ),
        );
        let mut records = Group::new("backup_records")
            .empty(IconName::Inbox, "backup_empty")
            .action(
                Button::new("backup-export")
                    .debug_selector(|| "backup-export".into())
                    .icon(gpui_kit::assets::IconName::Upload)
                    .accessibility_label(tr("backup_export"))
                    .tooltip(tr(
                        if self
                            .settings
                            .as_ref()
                            .is_some_and(|settings| settings.passphrase_configured)
                        {
                            "backup_export"
                        } else {
                            "backup_passphrase_required"
                        },
                    ))
                    .loading(
                        self.pending
                            && self.request.as_ref().is_some_and(|request| {
                                matches!(request.command, Command::ExportBackup { .. })
                            }),
                    )
                    .disabled(
                        locked
                            || destination.is_none()
                            || !self
                                .settings
                                .as_ref()
                                .is_some_and(|settings| settings.passphrase_configured),
                    )
                    .on_click(cx.listener(|panel, _, window, cx| {
                        if let Some(settings) = &panel.settings {
                            panel.submit(
                                Command::ExportBackup {
                                    expected_revision: settings.revision,
                                },
                                window,
                                cx,
                            );
                        }
                    })),
            );
        if let Some(page) = &self.page {
            for record in &page.records {
                let id = record.id.clone();
                let record = record.clone();
                let open = record.clone();
                let owner = cx.entity();
                records = records.child(
                    Entry::new(
                        format!("backup-row-{id}"),
                        v_flex()
                            .gap_1()
                            .child(
                                chrono::DateTime::from_timestamp_millis(
                                    record.created_ms.min(i64::MAX as u64) as i64,
                                )
                                .map(|time| {
                                    time.with_timezone(&chrono::Local)
                                        .format("%Y-%m-%d %H:%M")
                                        .to_string()
                                })
                                .unwrap_or_else(|| record.id.clone()),
                            )
                            .child(div().text_color(cx.theme().muted_foreground).child(format!(
                                "{:.1} MiB",
                                record.size_bytes as f64 / (1024. * 1024.)
                            ))),
                    )
                    .action(
                        Button::new(SharedString::from(format!("backup-restore-{id}")))
                            .debug_selector(move || format!("backup-restore-{id}"))
                            .icon(gpui_kit::assets::IconName::Download)
                            .accessibility_label(tr("backup_restore"))
                            .disabled(locked)
                            .on_click(move |_, window, cx| {
                                restore::open(owner.clone(), open.clone(), window, cx)
                            }),
                    ),
                );
            }
        } else if self.settings.is_none() || destination.is_some() {
            records = records.child(if self.loading {
                div()
                    .relative()
                    .w_full()
                    .h_24()
                    .child(crate::ui::pending::overlay("backup-records-loading", cx))
                    .into_any_element()
            } else {
                div()
                    .h_24()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        crate::feedback::retry("backup-load-retry")
                            .debug_selector(|| "backup-load-retry".into())
                            .disabled(!self.active || self.pending)
                            .on_click(cx.listener(|panel, _, window, cx| panel.load(window, cx))),
                    )
                    .into_any_element()
            });
        }
        v_flex().gap_6().child(configuration).child(records).when(
            self.page.as_ref().is_some_and(|page| page.partial),
            |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("backup_partial")),
                )
            },
        )
    }
}
