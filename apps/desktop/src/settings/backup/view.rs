use super::*;
use crate::{
    settings::{entry::Entry, group::Group},
    tr,
};
use gpui_kit::{
    component::button::{Button, ButtonVariants},
    prelude::FluentBuilder as _,
};

fn storage(settings: Option<&Settings>) -> SharedString {
    let Some(settings) = settings else {
        return tr("backup_not_configured");
    };
    let mut names = Vec::new();
    if settings.local.as_ref().is_some_and(|local| local.enabled) {
        names.push(tr("backup_local").to_string());
    }
    if settings.online_enabled
        && let Some(destination) = &settings.destination
    {
        names.push(
            tr(match destination.kind {
                sailry_protocol::backup::Kind::Webdav => "backup_webdav",
                sailry_protocol::backup::Kind::S3 => "backup_s3",
            })
            .to_string(),
        );
    }
    if names.is_empty() {
        tr("backup_not_configured")
    } else {
        names.join(" · ").into()
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.locked();
        let passphrase = self
            .settings
            .as_ref()
            .is_some_and(|settings| settings.passphrase_configured);
        let target = self.settings.as_ref().is_some_and(|settings| {
            settings.local.as_ref().is_some_and(|local| local.enabled)
                || settings.online_enabled && settings.destination.is_some()
        });
        let configure = cx.entity();
        let restore = cx.entity();
        let export_setup = cx.entity();
        let backup_setup = cx.entity();
        let archives = cx.entity();
        let delete = cx.entity();
        let removed = sessions::revisions(&self.sessions, true);
        let mut rows = Group::new("settings_backup").heading(false).child(
            Entry::new("backup-storage-row", tr("backup_destination"))
                .control(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(storage(self.settings.as_ref())),
                )
                .action(
                    Button::new("backup-configure")
                        .label(tr("backup_configure"))
                        .disabled(locked)
                        .on_click(move |_, window, cx| editor::open(configure.clone(), window, cx)),
                ),
        );
        if let Some(editor) = &self.editor {
            rows = rows.children(editor.update(cx, |editor, cx| editor.automatic(locked, cx)));
        }
        rows = rows
            .child(
                Entry::new("backup-export-row", tr("backup_export_data")).action(
                    Button::new("backup-export")
                        .label(tr(if passphrase {
                            "backup_export"
                        } else {
                            "backup_configure"
                        }))
                        .disabled(locked)
                        .on_click(move |_, window, cx| {
                            if passphrase {
                                export_setup
                                    .update(cx, |panel, cx| panel.choose_export(window, cx));
                            } else {
                                editor::open(export_setup.clone(), window, cx);
                            }
                        }),
                ),
            )
            .child(
                Entry::new("backup-now-row", tr("backup_now")).action(
                    Button::new("backup-now")
                        .label(tr(if target && passphrase {
                            "backup_now"
                        } else {
                            "backup_configure"
                        }))
                        .disabled(locked)
                        .on_click(move |_, window, cx| {
                            if !target || !passphrase {
                                editor::open(backup_setup.clone(), window, cx);
                                return;
                            }
                            backup_setup.update(cx, |panel, cx| {
                                if let Some(settings) = &panel.settings {
                                    panel.submit(
                                        Command::ExportBackup {
                                            expected_revision: settings.revision,
                                        },
                                        window,
                                        cx,
                                    );
                                }
                            });
                        }),
                ),
            )
            .child(
                Entry::new("backup-import-row", tr("backup_import")).action(
                    Button::new("backup-import")
                        .label(tr("backup_restore"))
                        .disabled(locked)
                        .on_click(move |_, window, cx| {
                            restore::open_file(restore.clone(), window, cx)
                        }),
                ),
            )
            .child(
                Entry::new("backup-storage-restore-row", tr("backup_restore_storage")).action(
                    Button::new("backup-storage-restore")
                        .label(tr("backup_restore"))
                        .disabled(locked || !target)
                        .on_click(
                            cx.listener(|panel, _, window, cx| panel.open_records(window, cx)),
                        ),
                ),
            )
            .child(
                Entry::new("backup-archives-row", tr("backup_archived_sessions")).action(
                    Button::new("backup-archives")
                        .label(tr("backup_manage"))
                        .disabled(locked)
                        .on_click(move |_, window, cx| {
                            sessions::open(archives.clone(), window, cx)
                        }),
                ),
            )
            .child(
                Entry::new("backup-archive-all-row", tr("backup_archive_all")).action(
                    Button::new("backup-archive-all")
                        .label(tr("backup_archive_all_action"))
                        .disabled(locked || sessions::revisions(&self.sessions, false).is_empty())
                        .on_click(cx.listener(|panel, _, window, cx| {
                            panel.submit(
                                Command::ArchiveSessions {
                                    expected: sessions::revisions(&panel.sessions, false),
                                },
                                window,
                                cx,
                            );
                        })),
                ),
            )
            .child(
                Entry::new("backup-delete-all-row", tr("backup_delete_all")).action(
                    Button::new("backup-delete-all")
                        .danger()
                        .label(tr("backup_delete_all_action"))
                        .disabled(locked || removed.is_empty())
                        .on_click(move |_, window, cx| {
                            sessions::confirm(delete.clone(), removed.clone(), window, cx);
                        }),
                ),
            );
        v_flex()
            .gap_3()
            .child(rows)
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("backup_node_online")),
            )
            .when(self.settings.is_none() && !self.loading, |view| {
                view.child(
                    crate::feedback::retry("backup-load-retry")
                        .disabled(!self.active)
                        .on_click(cx.listener(|panel, _, window, cx| panel.load(window, cx))),
                )
            })
    }
}
