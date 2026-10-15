use super::*;
use sailry_protocol::ErrorCode;

#[derive(Default)]
pub(in crate::conversation::live) struct Actions {
    pub(super) open: bool,
    observed: Vec<ApprovalId>,
    settled: std::collections::BTreeSet<ApprovalId>,
    pub(super) pending: bool,
    pub(in crate::conversation::live) attempt: Option<Request>,
    pub(in crate::conversation::live) error: Option<&'static str>,
    task: Option<Task<()>>,
    pub(super) children: BTreeMap<SessionId, super::watch::Child>,
    pub(super) drafts: BTreeMap<ApprovalId, super::draft::Draft>,
}

impl Actions {
    pub(in crate::conversation::live) fn observe(&mut self, history: &History) {
        let captured: Vec<_> = history
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
        // Only confirmed business results bridge the interval before their stream event.
        self.settled.retain(|id| captured.contains(id));
        let pending: Vec<_> = captured
            .into_iter()
            .filter(|id| !self.settled.contains(id))
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

    pub(super) fn resolved(&self, id: ApprovalId) -> bool {
        self.settled.contains(&id)
    }
}

impl View {
    pub(super) fn approval_target(&self, id: ApprovalId) -> Option<&Approval> {
        if self.readonly()
            || self.approvals.resolved(id)
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
            let key = if !self.connected()
                && [&self.node.error, &self.history.error]
                    .into_iter()
                    .flatten()
                    .any(|error| error.code == ErrorCode::Busy)
            {
                "live_read_busy"
            } else if !self.connected() {
                "chat_reconnecting"
            } else if self.approvals.pending || self.approvals.attempt.is_some() {
                "approval_busy"
            } else {
                "approval_closed"
            };
            crate::feedback::toast(
                window,
                tr(key),
                gpui_kit::component::notification::Notification::error(tr(key)),
                cx,
            );
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
        // The conversation owns the captured request and task, not the popup.
        self.approvals.open = false;
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
        self.approvals.pending = true;
        self.approvals.error = None;
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        self.approvals.task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = job.await;
            let _ = view.update_in(cx, |view, _window, cx| {
                view.approvals.pending = false;
                match result {
                    Ok(Ok(Output::Approval(value)))
                        if value.id == approval
                            && value.session == session
                            && value.state != ApprovalState::Pending =>
                    {
                        view.approvals.settled.insert(approval);
                        view.approvals.attempt = None;
                        view.approvals.drafts.remove(&approval);
                        view.approvals.observe(&view.history);
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
                cx.notify();
            });
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::conversation::{ApprovalSource, Snapshot};

    #[core::prelude::v1::test]
    fn confirmed_receipt_bridges_pending_stream() {
        let session = sailry_protocol::SessionId::new();
        let approval = Approval {
            id: ApprovalId::new(),
            session,
            turn: sailry_protocol::TurnId::new(),
            entry: "entry".into(),
            index: 0,
            state: ApprovalState::Pending,
            source: ApprovalSource::User,
            access: None,
            startup: None,
            ssh: None,
            can_remember: false,
            prefix: None,
            choice: None,
            reason: None,
        };
        let mut history = History {
            snapshot: Some(std::sync::Arc::new(Snapshot {
                node: sailry_protocol::NodeId([1; 32]),
                sequence: 1,
                page: std::sync::Arc::new(Page {
                    session,
                    revision: 1,
                    entries: Vec::new(),
                    runs: Vec::new(),
                    next_before: None,
                    queue: Default::default(),
                    approvals: vec![approval.clone()],
                    questions: Vec::new(),
                    children: Vec::new(),
                }),
                missing: Vec::new(),
                drafts: Vec::new(),
                statistics: Default::default(),
                artifacts: Default::default(),
            })),
            ..Default::default()
        };
        let mut actions = Actions::default();
        actions.observe(&history);
        assert!(actions.open);
        assert!(!actions.resolved(approval.id));
        actions.attempt = Some(Request::new(
            sailry_protocol::NodeId([1; 32]),
            Command::ResolveApproval {
                session,
                approval: approval.id,
                decision: Decision::Approve,
            },
        ));
        actions.observe(&history);
        assert!(!actions.resolved(approval.id));

        actions.settled.insert(approval.id);
        actions.attempt = None;
        actions.observe(&history);
        assert!(actions.resolved(approval.id));
        assert!(!actions.open);
        assert_eq!(
            history.snapshot.as_ref().unwrap().page.approvals[0],
            approval
        );

        let snapshot = std::sync::Arc::make_mut(history.snapshot.as_mut().unwrap());
        std::sync::Arc::make_mut(&mut snapshot.page).approvals[0].state = ApprovalState::Approved;
        actions.observe(&history);
        assert!(!actions.resolved(approval.id));
        assert!(!actions.open);
    }
}
