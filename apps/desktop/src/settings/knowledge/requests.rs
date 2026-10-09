use super::*;
use sailry_protocol::{Command, ErrorCode, Fault, Output, RequestOutcome};

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
                .execute(binding.client.prepare(Command::ReadKnowledgeSettings))
                .await?;
            let models = binding
                .client
                .execute(binding.client.prepare(Command::ReadKnowledgeModels))
                .await?;
            let collections = binding
                .client
                .execute(binding.client.prepare(Command::ListKnowledgeCollections))
                .await?;
            let snapshot = binding
                .client
                .execute(binding.client.prepare(Command::Snapshot))
                .await?;
            Ok::<_, Fault>((settings, models, collections, snapshot))
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.loading = false;
                if !panel.active {
                    return;
                }
                if panel.generation != generation {
                    panel.load(window, cx);
                    return;
                }
                match result {
                    Ok(Ok((
                        Output::KnowledgeSettings(settings),
                        Output::KnowledgeModels(models),
                        Output::KnowledgeCollections(collections),
                        Output::Snapshot(snapshot),
                    ))) if snapshot.node == panel.binding.client.target() => {
                        let dirty = panel.dirty(cx);
                        let selected = if dirty {
                            panel.model.read(cx).selected_value().copied().flatten()
                        } else {
                            settings.model
                        };
                        panel.update_choices(models, selected, window, cx);
                        if dirty
                            && !panel.rebase
                            && panel
                                .saved
                                .as_ref()
                                .is_some_and(|saved| saved.revision != settings.revision)
                        {
                            panel.error = Some("knowledge_conflict");
                        } else {
                            panel.apply(settings, dirty, window, cx);
                            if panel.rebase {
                                panel.error = None;
                            }
                        }
                        panel.collections = collections;
                        panel.worktrees = snapshot.worktrees;
                        panel.rebase = false;
                    }
                    Ok(Err(error)) => panel.error = Some(error_key(error.code)),
                    _ => panel.error = Some("knowledge_failed"),
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

    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings(cx) {
            self.submit(Command::SaveKnowledgeSettings(settings), window, cx);
        }
    }

    pub(in crate::settings) fn submit(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active {
            self.error = Some("knowledge_target_changed");
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
        let notice = match &request.command {
            Command::InstallKnowledgeModel { .. } => "knowledge_installed",
            Command::DeleteKnowledgeModel { .. } => "knowledge_deleted",
            Command::DeleteKnowledgeCollection { .. } => "knowledge_deleted",
            Command::RebuildKnowledgeCollection { .. } => "knowledge_rebuild_started",
            Command::PauseKnowledgeCollection { paused: true, .. } => "knowledge_pause_requested",
            Command::PauseKnowledgeCollection { paused: false, .. } => "knowledge_resume_requested",
            _ => "knowledge_saved",
        };
        let registering = matches!(request.command, Command::RegisterKnowledgeModel { .. });
        self.pending = true;
        self.error = None;
        let binding = self.binding.clone();
        let runtime = binding.runtime.clone();
        let job = runtime.spawn(async move { execute(&binding, request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                match result {
                    Ok((Ok(output), _)) => {
                        panel.request = None;
                        if let Output::KnowledgeSettings(settings) = output {
                            panel.apply(settings, false, window, cx);
                        } else if registering && let Output::KnowledgeModel { model } = output {
                            panel.submit(Command::InstallKnowledgeModel { model }, window, cx);
                            return;
                        }
                        crate::feedback::info("", &crate::tr(notice), window, cx);
                        panel.load(window, cx);
                    }
                    Ok((Err(error), confirmed)) => {
                        let uncertain = !confirmed || error.code == ErrorCode::OutcomeUnknown;
                        if !uncertain {
                            panel.request = None;
                        }
                        if uncertain || error.code != ErrorCode::Cancelled {
                            panel.error = Some(if uncertain {
                                "knowledge_unknown"
                            } else {
                                error_key(error.code)
                            });
                        }
                        if !uncertain {
                            panel.load(window, cx);
                        }
                    }
                    Err(_) => panel.error = Some("knowledge_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::settings) fn cancel(
        &mut self,
        model: Model,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active || self.cancelling.is_some() {
            return;
        }
        self.cancelling = Some(model);
        let binding = self.binding.clone();
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(Command::CancelKnowledgeModel { model }),
                )
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.cancelling = None;
                match result {
                    Ok(Ok(_)) => crate::feedback::info(
                        "",
                        &crate::tr("knowledge_cancel_requested"),
                        window,
                        cx,
                    ),
                    _ => panel.error = Some("knowledge_failed"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

pub(super) async fn execute(binding: &Binding, request: Request) -> (Result<Output, Fault>, bool) {
    let durable = request.command.durable();
    let result = binding.client.execute(request.clone()).await;
    if durable && result.is_err() {
        match binding.client.outcome(&request).await {
            Ok(RequestOutcome::Completed(result)) => (*result, true),
            _ => (result, false),
        }
    } else {
        (result, true)
    }
}

pub(super) fn error_key(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::RevisionConflict => "knowledge_conflict",
        ErrorCode::InvalidRequest => "knowledge_invalid",
        ErrorCode::PermissionDenied => "knowledge_denied",
        ErrorCode::OutcomeUnknown => "knowledge_unknown",
        _ => "knowledge_failed",
    }
}
