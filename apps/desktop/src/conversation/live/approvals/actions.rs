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
    pub(super) children: BTreeMap<SessionId, super::watch::Child>,
    pub(super) drafts: BTreeMap<ApprovalId, super::draft::Draft>,
}

impl Actions {
    #[cfg(test)]
    pub(in crate::conversation::live) fn pending_children(&self) -> usize {
        self.children
            .values()
            .filter(|child| {
                child.history.calls.iter().any(|call| {
                    call.approval
                        .as_ref()
                        .is_some_and(|approval| approval.state == ApprovalState::Pending)
                })
            })
            .count()
    }

    pub(in crate::conversation::live) fn observe(&mut self, history: &History) {
        let pending: Vec<_> = history
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.page.approvals)
            .chain(self.children.values().flat_map(|child| {
                child
                    .history
                    .snapshot
                    .iter()
                    .flat_map(|snapshot| &snapshot.page.approvals)
            }))
            .filter(|approval| approval.state == ApprovalState::Pending)
            .map(|approval| approval.id)
            .collect();
        if pending.iter().any(|id| !self.observed.contains(id)) {
            self.open = true;
        } else if pending.is_empty() && self.error.is_none() && self.attempt.is_none() {
            self.open = false;
        }
        self.observed = pending;
        let attempted = self
            .attempt
            .as_ref()
            .and_then(|request| match &request.command {
                Command::ResolveApproval { approval, .. } => Some(*approval),
                _ => None,
            });
        self.drafts
            .retain(|id, _| self.observed.contains(id) || attempted == Some(*id));
    }
}

impl View {
    pub(super) fn approval_target(&self, id: ApprovalId) -> Option<&Approval> {
        if self.readonly()
            || self.approvals.pending
            || self.approvals.attempt.is_some()
            || !self.connected()
        {
            return None;
        }
        let history = std::iter::once(&self.history)
            .chain(self.approvals.children.values().map(|child| &child.history))
            .find(|history| {
                history.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot
                        .page
                        .approvals
                        .iter()
                        .any(|approval| approval.id == id)
                })
            })?;
        let snapshot = history.snapshot.as_ref()?;
        let approval = snapshot
            .page
            .approvals
            .iter()
            .find(|approval| approval.id == id && approval.state == ApprovalState::Pending)?;
        if !history.connected
            || !snapshot
                .page
                .runs
                .iter()
                .any(|run| run.turn == approval.turn && run.status == Status::Running)
        {
            return None;
        }
        Some(approval)
    }

    pub(in crate::conversation::live) fn resolve_approval(
        &mut self,
        id: ApprovalId,
        decision: Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(approval) = self.approval_target(id) else {
            return;
        };
        match &decision {
            Decision::ApproveSession if !approval.can_remember => return,
            Decision::ApprovePrefix(prefix)
                if !approval.can_remember || approval.prefix.as_ref() != Some(prefix) =>
            {
                return;
            }
            Decision::DenyWithReason(reason) if reason.trim().is_empty() => return,
            _ => {}
        }
        let session = approval.session;
        self.approvals.attempt = Some(self.binding.client.prepare(Command::ResolveApproval {
            session,
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
        if self.readonly() || self.approvals.pending || !self.connected() {
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
            let _ = view.update_in(cx, |view, window, cx| {
                view.approvals.pending = false;
                match result {
                    Ok(Ok(Output::Approval(value)))
                        if value.id == approval && value.session == session =>
                    {
                        view.approvals.attempt = None;
                        view.approvals.drafts.remove(&approval);
                        let message = tr(if value.state == ApprovalState::Denied {
                            "approval_rejected"
                        } else {
                            "approval_approved"
                        });
                        crate::feedback::toast(
                            window,
                            message.clone(),
                            gpui_kit::component::notification::Notification::success(message),
                            cx,
                        );
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
