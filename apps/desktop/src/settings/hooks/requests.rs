use super::*;
use sailry_protocol::{Command, ErrorCode, Output};

impl Panel {
    pub(super) fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || self.pending || self.request.is_some() || !self.active {
            return;
        }
        self.loading = true;
        self.error = None;
        let binding = self.binding.clone();
        let worktree = self.worktree;
        let job = binding.runtime.spawn(async move {
            let settings = binding
                .client
                .execute(binding.client.prepare(Command::ReadHooks { worktree }))
                .await?;
            let catalog = binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(Command::ReadWorktreeCatalog { worktree }),
                )
                .await?;
            Ok::<_, sailry_protocol::Fault>((settings, catalog))
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.loading = false;
                match result {
                    Ok(Ok((Output::HookSettings(state), Output::ProjectCatalog(catalog))))
                        if state.worktree == panel.worktree =>
                    {
                        panel.root = catalog
                            .worktrees
                            .iter()
                            .find(|tree| tree.id == panel.worktree)
                            .map(|tree| tree.path.clone());
                        panel.sandboxed = state.sandboxed;
                        panel.apply(state.settings, window, cx);
                    }
                    _ => panel.error = Some("hooks_failed"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn submit(
        &mut self,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active {
            self.error = Some("hooks_target_changed");
            cx.notify();
            return;
        }
        if self.locked() {
            return;
        }
        self.request = Some(self.binding.client.prepare(Command::SaveHooks {
            worktree: self.worktree,
            expected_revision: settings.revision,
            settings,
        }));
        self.retry(window, cx);
    }

    pub(super) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.active {
            return;
        }
        let Some(request) = self.request.clone() else {
            return;
        };
        self.pending = true;
        self.error = None;
        let binding = self.binding.clone();
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                match result {
                    Ok(Ok(Output::HookSettings(state))) if state.worktree == panel.worktree => {
                        panel.request = None;
                        panel.sandboxed = state.sandboxed;
                        panel.apply(state.settings, window, cx);
                        window.push_notification(crate::tr("hooks_saved"), cx);
                    }
                    Ok(Err(error)) => {
                        if !matches!(
                            error.code,
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
                        ) {
                            panel.request = None;
                        }
                        panel.error = Some(match error.code {
                            ErrorCode::Conflict | ErrorCode::RevisionConflict => "hooks_conflict",
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable => "hooks_unknown",
                            ErrorCode::InvalidRequest => "hooks_invalid",
                            ErrorCode::PermissionDenied => "hooks_denied",
                            _ => "hooks_failed",
                        });
                    }
                    _ => panel.error = Some("hooks_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
