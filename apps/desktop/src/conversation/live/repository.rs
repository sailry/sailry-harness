//! Checkout identity is core location metadata; feature UI is supplied by packages.
use super::*;

#[derive(Default)]
pub(super) struct State {
    scope: Option<(sailry_protocol::NodeId, WorktreeId)>,
    stop: Option<CancellationToken>,
    task: Option<Task<()>>,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop {
            stop.cancel();
        }
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
        if !eligible
            || self.repository.scope
                != self
                    .binding
                    .worktree
                    .map(|tree| (self.binding.client.target(), tree))
        {
            self.repository = State::default();
        }
        let Some(worktree) = self.binding.worktree.filter(|_| eligible) else {
            return;
        };
        if self.repository.task.is_some()
            || (matches!(event, Refresh::Snapshot)
                && self.repository.scope == Some((self.binding.client.target(), worktree)))
        {
            return;
        }
        let node = self.binding.client.target();
        self.repository.scope = Some((node, worktree));
        let stop = self
            .repository
            .stop
            .get_or_insert_with(|| self.stop.child_token())
            .clone();
        let current = stop.clone();
        let client = self.binding.client.clone();
        let job = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = client.execute(client.prepare(Command::InspectGit {worktree})) => Some(result),
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
                view.repository.task = None;
                if let Ok(Some(Ok(Output::GitStatus(status)))) = result {
                    view.git = status.kind != sailry_protocol::RepositoryKind::Directory;
                    view.binding.branch = status
                        .branch
                        .or_else(|| status.head.map(|head| head.chars().take(8).collect()))
                        .unwrap_or_else(|| tr("composer_branch_none").to_string())
                        .into();
                    cx.notify();
                }
            });
        }));
    }
}
