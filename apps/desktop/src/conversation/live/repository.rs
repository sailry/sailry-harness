//! Checkout identity is core location metadata; feature UI is supplied by packages.
use super::*;
mod inspection;
use inspection::{Inspection, Kind};

#[derive(Default)]
pub(super) struct State {
    inspection: Inspection,
    stop: Option<CancellationToken>,
    task: Option<Task<()>>,
}

impl State {
    pub(super) fn is_git(&self) -> bool {
        self.inspection.kind() == Kind::Repository
    }
    pub(super) fn kind(&self) -> &'static str {
        self.inspection.kind().as_str()
    }
    fn cancel(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
        self.task = None;
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Clone, Copy)]
pub(super) enum Refresh {
    Snapshot,
    History,
    Focus,
    Location,
}

impl View {
    pub(super) fn refresh_repository(&mut self, event: Refresh, cx: &mut Context<Self>) {
        let eligible = self.resource.is_none()
            && self.assistant.is_none()
            && self.binding.project.is_some()
            && self.node.snapshot.is_some();
        let node = self.binding.client.target();
        let scope = self
            .binding
            .worktree
            .filter(|_| eligible)
            .map(|tree| (node, tree));
        if self
            .repository
            .inspection
            .select(scope, self.node.connected)
        {
            self.repository.cancel();
        }
        let Some((_, worktree)) = scope else { return };
        let Some(generation) = self
            .repository
            .inspection
            .begin(!matches!(event, Refresh::Snapshot))
        else {
            return;
        };
        let stop = self.stop.child_token();
        self.repository.stop = Some(stop.clone());
        let current = stop.clone();
        let client = self.binding.client.clone();
        let request = client.prepare(Command::InspectGit { worktree });
        let request_id = request.id;
        let job = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = client.execute(request) => Some(result),
            }
        });
        self.repository.task = Some(cx.spawn(async move |view, cx| {
            let result = job.await;
            let _ = view.update(cx, |view, cx| {
                if current.is_cancelled()
                    || view.binding.client.target() != node
                    || view.binding.worktree != Some(worktree)
                {
                    return;
                }
                let result = match result {
                    Ok(Some(Ok(Output::GitStatus(status)))) => Ok(status),
                    Ok(Some(Err(error))) => Err(error),
                    Ok(None) => return,
                    _ => Err(sailry_protocol::Fault::new(
                        sailry_protocol::ErrorCode::Internal,
                        "Git inspection returned no status",
                    )),
                };
                if !view
                    .repository
                    .inspection
                    .finish(generation, result.as_ref().ok().map(|status| status.kind))
                {
                    return;
                }
                view.repository.task = None;
                match result {
                    Ok(status) => {
                        view.binding.branch = status
                            .branch
                            .or_else(|| status.head.map(|head| head.chars().take(8).collect()))
                            .unwrap_or_else(|| tr("composer_branch_none").to_string())
                            .into();
                    }
                    Err(error)
                        if error.code != sailry_protocol::ErrorCode::Cancelled
                            && view.repository.inspection.report_failure() =>
                    {
                        use sailry_protocol::notification::{SystemDiagnostic, SystemSource};
                        let mut report = SystemDiagnostic::new(
                            request_id,
                            SystemSource::GitInspection,
                            error.code,
                        );
                        report.request = Some(request_id);
                        report.worktree = Some(worktree);
                        report.session = view.session.as_ref().map(|session| session.id);
                        report.operation = Some("InspectGit".into());
                        let client = view.binding.client.clone();
                        view.binding.runtime.spawn(async move {
                            let _ = client.report_diagnostic(report).await;
                        });
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        }));
    }
}
