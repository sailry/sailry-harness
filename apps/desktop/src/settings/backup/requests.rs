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
            binding
                .client
                .execute(binding.client.prepare(Command::ReadBackupSettings))
                .await
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
                    Ok(Ok(Output::BackupSettings(settings))) => {
                        if let Some(editor) = panel.editor.as_ref().and_then(WeakEntity::upgrade) {
                            editor.update(cx, |editor, cx| {
                                editor.observe(&settings, panel.rebase, window, cx)
                            });
                        }
                        panel.settings = Some(settings);
                        panel.error = None;
                        panel.rebase = false;
                        panel.list(window, cx);
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

    fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.settings.as_ref() else {
            return;
        };
        if settings.destination.is_none() {
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
                if generation == panel.generation
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
        let notice = match request.command {
            Command::ExportBackup { .. } => "backup_exported",
            Command::RestoreBackup { .. } => "backup_restored",
            _ => "backup_saved",
        };
        let binding = self.binding.clone();
        self.pending = true;
        self.error = None;
        let runtime = binding.runtime.clone();
        let job = runtime.spawn(async move {
            super::super::knowledge::requests::execute(&binding, request).await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                match result {
                    Ok((Ok(output), _)) => {
                        panel.request = None;
                        if let Output::BackupSettings(settings) = output {
                            if let Some(editor) =
                                panel.editor.as_ref().and_then(WeakEntity::upgrade)
                            {
                                editor.update(cx, |editor, cx| {
                                    editor.accept(settings.clone(), window, cx)
                                });
                            }
                            panel.settings = Some(settings);
                        }
                        crate::feedback::info("", &crate::tr(notice), window, cx);
                        panel.load(window, cx);
                    }
                    Ok((Err(error), confirmed)) => {
                        let uncertain = !confirmed || error.code == ErrorCode::OutcomeUnknown;
                        if !uncertain {
                            panel.request = None;
                        }
                        if let Some(editor) = panel.editor.as_ref().and_then(WeakEntity::upgrade) {
                            editor.update(cx, |editor, cx| {
                                editor.conflict = error.code == ErrorCode::RevisionConflict;
                                cx.notify();
                            });
                        }
                        panel.error = Some(if uncertain {
                            "backup_unknown"
                        } else {
                            error_key(error.code)
                        });
                    }
                    Err(_) => panel.error = Some("backup_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
