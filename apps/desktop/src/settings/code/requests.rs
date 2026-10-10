use super::*;
use sailry_protocol::{Command, ErrorCode, Output, RequestOutcome};

impl Panel {
    pub(super) fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || self.pending || self.request.is_some() {
            return;
        }
        self.loading = true;
        self.error = None;
        let binding = self.binding.clone();
        let worktree = self.worktree;
        let job = binding.runtime.spawn(async move {
            let settings = binding
                .client
                .execute(binding.client.prepare(Command::ReadCodeSettings))
                .await?;
            let packages = binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(Command::ListCodePackages { worktree }),
                )
                .await?;
            let installs = binding
                .client
                .execute(binding.client.prepare(Command::ListCodeInstallations))
                .await?;
            let rules = binding
                .client
                .execute(binding.client.prepare(Command::ReadCodeRules))
                .await?;
            Ok::<_, sailry_protocol::Fault>((settings, packages, installs, rules))
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.loading = false;
                match result {
                    Ok(Ok((
                        Output::CodeSettings(settings),
                        Output::CodePackages(workspace),
                        Output::CodeInstallations(installs),
                        Output::CodeRules(rules),
                    ))) if workspace.worktree == panel.worktree => {
                        if panel.dirty()
                            && panel
                                .saved
                                .as_ref()
                                .is_some_and(|saved| saved.revision != settings.revision)
                        {
                            panel.error = Some("code_conflict");
                        } else if !panel.dirty() {
                            panel.saved = Some(settings.clone());
                            panel.draft = Some(settings);
                        }
                        panel.workspace = Some(workspace);
                        if panel.stop.is_none() {
                            panel.installations = installs;
                        }
                        panel.rules = Some(rules);
                        if panel.active {
                            panel.watch(window, cx);
                        }
                    }
                    Ok(Err(error)) => panel.error = Some(error_key(error.code, false)),
                    _ => panel.error = Some("code_failed"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dirty() || self.error == Some("code_conflict") {
            return;
        }
        let (Some(settings), Some(saved)) = (self.draft.clone(), self.saved.as_ref()) else {
            return;
        };
        self.submit(
            Command::SaveCodeSettings {
                settings,
                expected_revision: saved.revision,
            },
            window,
            cx,
        );
    }

    pub(super) fn submit(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active {
            self.error = Some("code_target_changed");
            cx.notify();
            return;
        }
        if self.pending || self.request.is_some() {
            return;
        }
        self.request = Some(self.binding.client.prepare(command));
        self.retry(window, cx);
    }

    pub(super) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.active {
            return;
        }
        let Some(request) = self.request.clone() else {
            return;
        };
        let submitted = match &request.command {
            Command::SaveCodeSettings { settings, .. } => Some(settings.clone()),
            _ => None,
        };
        let durable = request.command.durable();
        let notice = match &request.command {
            Command::InstallCodeServer { .. } => "code_installed",
            Command::CancelCodeInstall { .. } => "code_cancel_requested",
            Command::StartCodeServer { .. } => "code_started",
            Command::StopCodeServer { .. } => "code_stopped",
            Command::UninstallCodeServer { .. } => "code_uninstalled",
            _ => "code_saved",
        };
        self.pending = true;
        self.error = None;
        let binding = self.binding.clone();
        let job = binding.runtime.spawn(async move {
            let result = binding.client.execute(request.clone()).await;
            if durable && result.is_err() {
                match binding.client.outcome(&request).await {
                    Ok(RequestOutcome::Completed(result)) => (*result, true),
                    _ => (result, false),
                }
            } else {
                (result, true)
            }
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                match result {
                    Ok((Ok(output), _)) => {
                        panel.request = None;
                        match output {
                            Output::CodeSettings(settings) => {
                                if panel.draft == submitted {
                                    panel.draft = Some(settings.clone());
                                } else if let Some(draft) = panel.draft.as_mut() {
                                    draft.revision = settings.revision;
                                }
                                panel.saved = Some(settings);
                            }
                            Output::CodeInstallation(installation) => {
                                panel.accept_install(installation)
                            }
                            Output::CodeServer(server) => {
                                panel.servers.retain(|current| {
                                    current.startup.server != server.startup.server
                                });
                                panel.servers.push(server);
                            }
                            Output::CodeRules(rules) => panel.rules = Some(rules),
                            _ => {}
                        }
                        window.push_notification(crate::tr(notice), cx);
                        if panel.active {
                            if submitted.is_some() {
                                panel.save(window, cx);
                            }
                            panel.load(window, cx);
                        }
                    }
                    Ok((Err(error), confirmed)) => {
                        let uncertain =
                            durable && (!confirmed || error.code == ErrorCode::OutcomeUnknown);
                        if !uncertain {
                            panel.request = None;
                        }
                        if error.code == ErrorCode::Cancelled {
                            window.push_notification(crate::tr("code_cancelled"), cx);
                        } else {
                            panel.error = Some(error_key(error.code, uncertain));
                        }
                    }
                    Err(_) => {
                        if !durable {
                            panel.request = None;
                        }
                        panel.error = Some(if durable {
                            "code_unknown"
                        } else {
                            "code_failed"
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn cancel(&mut self, package: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active || self.cancelling.is_some() {
            return;
        }
        self.cancelling = Some(package.clone());
        let binding = self.binding.clone();
        let request = binding
            .client
            .prepare(Command::CancelCodeInstall { package });
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.cancelling = None;
                match result {
                    Ok(Ok(_)) => window.push_notification(crate::tr("code_cancel_requested"), cx),
                    _ => panel.error = Some("code_failed"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn accept_install(&mut self, installation: code::Installation) {
        if installation.receipt.is_some() && installation.status == code::InstallStatus::Ready {
            self.tab = 0;
        }
        self.installations
            .retain(|current| current.package != installation.package);
        self.installations.push(installation);
    }
}

fn error_key(code: ErrorCode, uncertain: bool) -> &'static str {
    match code {
        ErrorCode::RevisionConflict => "code_conflict",
        ErrorCode::InvalidRequest => "code_invalid",
        ErrorCode::PermissionDenied => "code_denied",
        ErrorCode::OutcomeUnknown | ErrorCode::Unavailable if uncertain => "code_unknown",
        _ => "code_failed",
    }
}
