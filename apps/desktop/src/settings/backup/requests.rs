use super::*;
use sailry_protocol::{ErrorCode, Output};

impl Panel {
    pub(super) fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        if self.loading || self.pending || self.request.is_some() {
            self.refresh = true;
            return;
        }
        self.loading = true;
        self.refresh = false;
        let binding = self.binding.clone();
        let generation = self.generation;
        let job = binding.runtime.spawn(async move {
            let settings = binding
                .client
                .execute(binding.client.prepare(Command::ReadBackupSettings))
                .await?;
            let snapshot = binding
                .client
                .execute(binding.client.prepare(Command::Snapshot))
                .await?;
            Ok::<_, sailry_protocol::Fault>((settings, snapshot))
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.loading = false;
                if !panel.active {
                    return;
                }
                if generation != panel.generation {
                    panel.load(window, cx);
                    return;
                }
                match result {
                    Ok(Ok((Output::BackupSettings(settings), Output::Snapshot(snapshot)))) => {
                        panel.sessions = snapshot.sessions;
                        if let Some(editor) = panel.editor.as_ref() {
                            editor.update(cx, |editor, cx| {
                                editor.observe(&settings, panel.rebase, window, cx)
                            });
                        }
                        if panel.editor.is_none() {
                            let editor =
                                editor::create(cx.weak_entity(), settings.clone(), window, cx);
                            cx.observe(&editor, |_, _, cx| cx.notify()).detach();
                            panel.editor = Some(editor);
                        }
                        panel.settings = Some(settings);
                        panel.error = None;
                        panel.rebase = false;
                        if panel.records_open {
                            panel.list(window, cx);
                        }
                    }
                    Ok(Err(error)) => panel.error = Some(error_key(error.code)),
                    _ => panel.error = Some("backup_failed"),
                }
                if panel.refresh {
                    panel.load(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.settings.as_ref() else {
            return;
        };
        if settings.destination.is_none() && settings.local.is_none() {
            self.page = None;
            return;
        }
        let revision = settings.revision;
        let generation = self.generation;
        let binding = self.binding.clone();
        self.loading = true;
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(binding.client.prepare(Command::ListBackups))
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.loading = false;
                if !panel.active {
                    return;
                }
                if panel.records_open
                    && generation == panel.generation
                    && panel
                        .settings
                        .as_ref()
                        .is_some_and(|settings| settings.revision == revision)
                {
                    match result {
                        Ok(Ok(Output::Backups(page))) => panel.page = Some(page),
                        Ok(Err(error)) => panel.error = Some(error_key(error.code)),
                        _ => panel.error = Some("backup_failed"),
                    }
                }
                if panel.refresh {
                    panel.load(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn submit(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.locked() {
            return false;
        }
        self.request = Some(self.binding.client.prepare(command));
        self.retry(window, cx);
        true
    }

    pub(super) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.active {
            return;
        }
        let Some(request) = self.request.clone() else {
            return;
        };
        let session_command = matches!(
            request.command,
            Command::ArchiveSessions { .. }
                | Command::RemoveSessions { .. }
                | Command::RemoveSession { .. }
                | Command::SetSessionArchived { .. }
        );
        let notice = match request.command {
            Command::ExportBackup { .. } => "backup_exported",
            Command::RestoreBackup { .. } | Command::RestoreBackupFile { .. } => "backup_restored",
            Command::ArchiveSessions { .. } => "backup_archived",
            Command::RemoveSessions { .. } | Command::RemoveSession { .. } => "backup_deleted",
            Command::SetSessionArchived { .. } => "backup_unarchived",
            _ => "backup_saved",
        };
        let binding = self.binding.clone();
        self.pending = true;
        self.error = None;
        let runtime = binding.runtime.clone();
        let job =
            runtime.spawn(async move { super::super::requests::execute(&binding, request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                match result {
                    Ok((Ok(output), _)) => {
                        panel.request = None;
                        if let Output::BackupSettings(settings) = output {
                            if let Some(editor) = panel.editor.as_ref() {
                                editor.update(cx, |editor, cx| {
                                    editor.accept(settings.clone(), window, cx)
                                });
                            }
                            panel.settings = Some(settings);
                        }
                        if panel.active {
                            crate::feedback::info("", &crate::tr(notice), window, cx);
                        }
                        panel.load(window, cx);
                    }
                    Ok((Err(error), confirmed)) => {
                        let uncertain = !confirmed || error.code == ErrorCode::OutcomeUnknown;
                        if !uncertain {
                            panel.request = None;
                        }
                        if !session_command && let Some(editor) = panel.editor.as_ref() {
                            editor.update(cx, |editor, cx| {
                                editor.conflict = error.code == ErrorCode::RevisionConflict;
                                cx.notify();
                            });
                        }
                        panel.error = Some(if uncertain {
                            if session_command {
                                "backup_sessions_unknown"
                            } else {
                                "backup_unknown"
                            }
                        } else if session_command {
                            match error.code {
                                ErrorCode::Busy => "backup_sessions_busy",
                                ErrorCode::RevisionConflict | ErrorCode::NotFound => {
                                    "session_changed"
                                }
                                _ => "backup_sessions_failed",
                            }
                        } else {
                            error_key(error.code)
                        });
                    }
                    Err(_) => {
                        panel.error = Some(if session_command {
                            "backup_sessions_unknown"
                        } else {
                            "backup_unknown"
                        })
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Panel {
    pub(super) fn choose_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let generation = self.generation;
        let selected = cx.prompt_for_new_path(
            &std::env::home_dir().unwrap_or_default(),
            Some("sailry-backup.age"),
        );
        cx.spawn_in(window, async move |owner, cx| {
            let selected = selected.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                if !panel.active || panel.generation != generation || panel.locked() {
                    return;
                }
                match selected {
                    Ok(Ok(Some(path))) => panel.download(path, window, cx),
                    Ok(Ok(None)) => (),
                    _ => {
                        crate::feedback::error("", &crate::tr("files_download_picker"), window, cx)
                    }
                }
            });
        })
        .detach();
    }

    fn download(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.settings.as_ref() else {
            return;
        };
        let revision = settings.revision;
        let generation = self.generation;
        let binding = self.binding.clone();
        let stop = CancellationToken::new();
        self.stop = Some(stop.clone());
        self.pending = true;
        let job = binding.runtime.clone().spawn(async move {
            let _cancel = stop.clone().drop_guard();
            let destination = crate::downloads::destination::Destination::prepare(path)?;
            let mut writer = tokio::fs::File::from_std(destination.writer()?);
            let output = tokio::select! {
                biased;
                _ = stop.cancelled() => return Err("files_download_cancelled"),
                result = binding.client.execute(binding.client.prepare(Command::DownloadBackup { expected_revision: revision })) => result.map_err(|error| error_key(error.code))?,
            };
            let Output::BackupDownload(download) = output else { return Err("backup_failed"); };
            binding.client.download_backup(&download, &mut writer, stop.clone(), |_| {}).await.map_err(|error| error_key(error.code))?;
            drop(writer);
            tokio::task::spawn_blocking(move || destination.publish(&stop)).await.map_err(|_| "backup_failed")?
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await.unwrap_or(Err("backup_failed"));
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                panel.stop = None;
                if !panel.active || panel.generation != generation {
                    return;
                }
                match result {
                    Ok(()) => crate::feedback::info("", &crate::tr("backup_exported"), window, cx),
                    Err("files_download_cancelled") => (),
                    Err(key) => crate::feedback::error("", &crate::tr(key), window, cx),
                }
                panel.load(window, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
