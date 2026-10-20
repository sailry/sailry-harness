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
        let label = v_flex()
            .gap_1()
            .children(self.settings.as_ref().into_iter().flat_map(|settings| {
                let local = settings
                    .local
                    .as_ref()
                    .map(|local| format!("{} · {}", tr("backup_local_enabled"), local.directory));
                let online = settings.destination.as_ref().map(|value| {
                    format!(
                        "{} · {} · {}",
                        tr("backup_online_enabled"),
                        value.endpoint,
                        value.root
                    )
                });
                local.into_iter().chain(online)
            }));
        let label = label.when(
            self.settings
                .as_ref()
                .is_some_and(|settings| settings.destination.is_none() && settings.local.is_none()),
            |label| label.child(tr("backup_not_configured")),
        );
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
        let passphrase = self
            .settings
            .as_ref()
            .is_some_and(|settings| settings.passphrase_configured);
        let target = self.settings.as_ref().is_some_and(|settings| {
            settings.local.as_ref().is_some_and(|local| local.enabled)
                || settings.online_enabled && settings.destination.is_some()
        });
        let import_owner = cx.entity();
        let mut records = Group::new("backup_records")
            .empty(IconName::Inbox, "backup_empty")
            .action(
                Button::new("backup-import")
                    .label(tr("backup_import"))
                    .disabled(locked)
                    .on_click(move |_, window, cx| {
                        restore::open_file(import_owner.clone(), window, cx)
                    }),
            )
            .action(
                Button::new("backup-export")
                    .label(tr("backup_export"))
                    .loading(self.pending && self.request.is_none())
                    .disabled(locked || !passphrase)
                    .on_click(cx.listener(|panel, _, window, cx| panel.choose_export(window, cx))),
            )
            .action(
                Button::new("backup-now")
                    .label(tr("backup_now"))
                    .loading(
                        self.pending
                            && self.request.as_ref().is_some_and(|request| {
                                matches!(request.command, Command::ExportBackup { .. })
                            }),
                    )
                    .disabled(locked || !target || !passphrase)
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
                let copies = page.locations.get(&id);
                let mut entry = Entry::new(
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
                );
                for (location, available, key) in [
                    (
                        sailry_protocol::backup::Location::Local,
                        copies.is_some_and(|copies| copies.local),
                        "backup_local_enabled",
                    ),
                    (
                        sailry_protocol::backup::Location::Online,
                        copies.is_none_or(|copies| copies.online),
                        "backup_online_enabled",
                    ),
                ] {
                    if available {
                        let owner = cx.entity();
                        let record = record.clone();
                        entry = entry.action(
                            Button::new(SharedString::from(format!(
                                "backup-restore-{id}-{location:?}"
                            )))
                            .label(tr(key))
                            .accessibility_label(format!("{} · {}", tr("backup_restore"), tr(key)))
                            .disabled(locked)
                            .on_click(move |_, window, cx| {
                                restore::open(owner.clone(), record.clone(), location, window, cx)
                            }),
                        );
                    }
                }
                records = records.child(entry);
            }
        } else if self.settings.is_none()
            || destination.is_some()
            || self
                .settings
                .as_ref()
                .is_some_and(|settings| settings.local.is_some())
        {
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
        v_flex()
            .gap_6()
            .child(configuration)
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("backup_node_online")),
            )
            .child(records)
            .when(
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
