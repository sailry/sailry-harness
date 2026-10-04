use super::*;
use sailry_protocol::ErrorCode;

#[derive(Default)]
pub(in crate::conversation::live) struct Actions {
    pub(super) open: bool,
    observed: Vec<ApprovalId>,
    pub(super) pending: bool,
    pub(in crate::conversation::live) attempt: Option<Request>,
    pub(in crate::conversation::live) error: Option<&'static str>,
    task: Option<Task<()>>,
}

impl Actions {
    pub(in crate::conversation::live) fn observe(&mut self, history: &History) {
        let pending: Vec<_> = history
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.page.approvals)
            .filter(|approval| approval.state == ApprovalState::Pending)
            .map(|approval| approval.id)
            .collect();
        if pending.iter().any(|id| !self.observed.contains(id)) {
            self.open = true;
        } else if pending.is_empty() && self.error.is_none() && self.attempt.is_none() {
            self.open = false;
        }
        self.observed = pending;
    }
}

impl View {
    pub(in crate::conversation::live) fn resolve_approval(
        &mut self,
        id: ApprovalId,
        decision: Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.approvals.pending || self.approvals.attempt.is_some() || !self.connected() {
            return;
        }
        let Some(snapshot) = &self.history.snapshot else {
            return;
        };
        let Some(approval) = snapshot
            .page
            .approvals
            .iter()
            .find(|approval| approval.id == id && approval.state == ApprovalState::Pending)
        else {
            return;
        };
        if self.session() != Some(approval.session)
            || !snapshot
                .page
                .runs
                .iter()
                .any(|run| run.turn == approval.turn && run.status == Status::Running)
        {
            return;
        }
        self.approvals.attempt = Some(self.binding.client.prepare(Command::ResolveApproval {
            session: approval.session,
            approval: id,
            decision,
        }));
        self.retry_approval(window, cx);
    }

    pub(in crate::conversation::live) fn retry_approval(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.approvals.pending || !self.connected() {
            return;
        }
        let Some(request) = self.approvals.attempt.clone() else {
            return;
        };
        let Command::ResolveApproval {
            session, approval, ..
        } = request.command
        else {
            return;
        };
        let client = self.binding.client.clone();
        self.approvals.open = true;
        self.approvals.pending = true;
        self.approvals.error = None;
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        self.approvals.task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = job.await;
            let _ = view.update_in(cx, |view, _, cx| {
                view.approvals.pending = false;
                match result {
                    Ok(Ok(Output::Approval(value)))
                        if value.id == approval && value.session == session =>
                    {
                        view.approvals.attempt = None;
                    }
                    Ok(Err(error)) => {
                        view.approvals.error = Some(match error.code {
                            ErrorCode::Conflict | ErrorCode::NotFound => "approval_closed",
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable => {
                                "chat_action_unknown"
                            }
                            _ => "chat_action_failed",
                        });
                        if !matches!(
                            error.code,
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable | ErrorCode::Busy
                        ) {
                            view.approvals.attempt = None;
                        }
                    }
                    _ => view.approvals.error = Some("chat_action_unknown"),
                }
                if view.approvals.error.is_some() {
                    view.approvals.open = true;
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
