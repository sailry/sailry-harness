use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, RequestId, Topic};
use std::sync::Mutex;

pub(super) fn restored(fixture: &Fixture) -> sailry_protocol::conversation::Snapshot {
    fixture.runtime.block_on(async {
        let client = Client::new(fixture.transport.clone());
        let mut stream = client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let update = tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .unwrap()
            .unwrap();
        let sailry_protocol::Update::ConversationSnapshot(snapshot) = update else {
            panic!("initial conversation snapshot expected")
        };
        let history = client
            .read_conversation(fixture.session.id, None, 100)
            .await
            .unwrap();
        assert_eq!(snapshot.page.approvals, history.page.approvals);
        snapshot
    })
}

struct Lost {
    inner: Arc<dyn Transport>,
    ids: Mutex<Vec<RequestId>>,
    decisions: Mutex<Vec<Decision>>,
}

struct Gate {
    inner: Arc<dyn Transport>,
    entered: CancellationToken,
    release: CancellationToken,
}

impl Transport for Gate {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::ResolveApproval { .. }) {
                self.entered.cancel();
                self.release.cancelled().await;
            }
            self.inner.dispatch(request).await
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
}

#[gpui::test]
fn closes_popup_before_decision_completion(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.start();
        let gate = Arc::new(Gate {
            inner: fixture.transport.clone(),
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(gate.clone()));
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        tap(visual, &format!("live-approval-approve-{id}"));
        wait(visual, |_| gate.entered.is_cancelled());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("pending-approvals").is_none());
        let captured = view.read_with(visual, |view, _| {
            assert!(!view.approvals.open);
            assert!(view.approvals.pending);
            assert!(!view.approvals.resolved(id));
            view.approvals.attempt.clone().unwrap()
        });
        assert!(matches!(captured.command, Command::ResolveApproval {
            approval, decision: Decision::Approve, ..
        } if approval == id));
        assert!(!fixture.directory.path().join("project/资料.txt").exists());
        gate.release.cancel();
        wait(visual, |cx| {
            !view.read(cx).approvals.pending && pending(&view, cx).is_some_and(|next| next != id)
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/资料.txt")).unwrap(),
            "完整内容 中文 🙂\n```rust\n"
        );
        let next = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        tap(visual, &format!("live-approval-reject-{next}"));
        wait(visual, |cx| view.read(cx).active().is_none());
        visual.update(|window, cx| {
            for key in ["approval_approved", "approval_rejected"] {
                assert_eq!(crate::feedback::tests::count(window, &tr(key), cx), 0);
            }
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

impl Transport for Lost {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let lose = if let Command::ResolveApproval { decision, .. } = &request.command {
                let mut ids = self.ids.lock().unwrap();
                ids.push(request.id);
                self.decisions.lock().unwrap().push(decision.clone());
                ids.len() == 1
            } else {
                false
            };
            let admission = self.inner.dispatch(request).await?;
            if !lose {
                return Ok(admission);
            }
            admission.completion.await.unwrap().unwrap();
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "injected missing response",
            ))
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
}

#[gpui::test]
fn retries_original_decision(cx: &mut TestAppContext) {
    init(cx);
    for (decision, remote) in [
        Decision::Approve,
        Decision::DenyWithReason("Read the file first 中文 🙂\nPreserve unrelated changes".into()),
    ]
    .into_iter()
    .flat_map(|decision| [false, true].map(|remote| (decision.clone(), remote)))
    {
        let fixture = Fixture::new(remote);
        fixture.start();
        let lost = Arc::new(Lost {
            inner: fixture.transport.clone(),
            ids: Mutex::new(Vec::new()),
            decisions: Mutex::new(Vec::new()),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(lost.clone()));
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        if let Decision::DenyWithReason(reason) = &decision {
            choices::reason(visual, id, reason);
            tap(visual, &format!("live-approval-reason-submit-{id}"));
        } else {
            tap(visual, &format!("live-approval-approve-{id}"));
        }
        wait(visual, |cx| {
            view.read(cx).approvals.error.is_some()
                && pending(&view, cx).is_some_and(|next| next != id)
        });
        visual.update(|window, cx| {
            assert_eq!(
                crate::feedback::tests::count(window, &tr("chat_action_unknown"), cx),
                1
            );
        });
        if let Decision::DenyWithReason(reason) = &decision {
            assert_eq!(
                view.read_with(visual, |view, cx| view.approvals.drafts[&id]
                    .input
                    .read(cx)
                    .value()),
                reason.as_str()
            );
            view.read_with(visual, |view, _| {
                assert_eq!(
                    view.history.calls[0]
                        .approval
                        .as_ref()
                        .unwrap()
                        .reason
                        .as_ref(),
                    Some(reason)
                );
            });
            assert!(!fixture.directory.path().join("project/资料.txt").exists());
        }
        let next = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        tap(visual, &format!("live-approval-reject-{next}"));
        assert_eq!(lost.ids.lock().unwrap().len(), 1);
        tap(visual, "live-approval-retry");
        wait(visual, |cx| {
            !view.read(cx).approvals.pending && view.read(cx).approvals.attempt.is_none()
        });
        let ids = lost.ids.lock().unwrap().clone();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(
            *lost.decisions.lock().unwrap(),
            vec![decision.clone(), decision]
        );
        assert!(!view.read_with(visual, |view, _| view.approvals.drafts.contains_key(&id)));
        assert!(!view.read_with(visual, |view, _| view.approvals.open));
        tap(visual, "live-approval-trigger");
        tap(visual, &format!("live-approval-reject-{next}"));
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).approvals.pending
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[cfg(unix)]
#[gpui::test]
fn requires_fresh_unrestricted_consent(cx: &mut TestAppContext) {
    use sailry_protocol::{
        Permission,
        conversation::{ApprovalChoice, ApprovalSource},
        process::Access,
    };
    init(cx);
    for (permission, remote) in [Permission::Ask, Permission::Project]
        .into_iter()
        .flat_map(|permission| [false, true].map(|remote| (permission, remote)))
    {
        let mut fixture = Fixture::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"command":"printf first > first.txt","description":"Inspect the execution environment","access":"unrestricted"}),
                ),
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"command":"printf second > second.txt","access":"unrestricted"}),
                ),
            ],
        );
        let mut config = fixture.session.config.clone();
        config.permission = permission;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        }) else {
            panic!("session expected")
        };
        fixture.session = session;
        fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Inspect with explicit access".into(),
        });
        let lost = Arc::new(Lost {
            inner: fixture.transport.clone(),
            ids: Mutex::new(Vec::new()),
            decisions: Mutex::new(Vec::new()),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(lost.clone()));
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let first = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        let initial = restored(&fixture);
        let mut projection =
            sailry_client::conversation::Projection::new(fixture.node.id(), fixture.session.id, 1);
        projection
            .apply(
                1,
                sailry_protocol::Update::ConversationSnapshot(initial.clone()),
            )
            .unwrap();
        assert_eq!(initial.page.approvals.len(), 1);
        assert_eq!(initial.page.approvals[0].id, first);
        assert_eq!(initial.page.approvals[0].state, ApprovalState::Pending);
        view.read_with(visual, |view, _| {
            let call = &view.history.calls[0];
            let approval = call.approval.as_ref().unwrap();
            assert_eq!(approval.access, Some(Access::Unrestricted));
            assert_eq!(approval.source, ApprovalSource::User);
            assert!(!approval.can_remember);
            assert!(approval.prefix.is_none());
            let details = details(call, &view.history.snapshot.as_ref().unwrap().page);
            assert_eq!(
                details.access.as_deref(),
                Some("Full filesystem and network access")
            );
            assert_eq!(
                details.description.as_deref(),
                Some("printf first > first.txt")
            );
            assert_eq!(
                details.notices,
                vec![SharedString::from("Inspect the execution environment")]
            );
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-access-{first}").into_boxed_str()
                ))
                .is_none()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-approve-options-{first}").into_boxed_str()
                ))
                .is_none()
        );
        assert!(!fixture.directory.path().join("project/first.txt").exists());
        tap(visual, &format!("live-approval-approve-{first}"));
        wait(visual, |cx| {
            view.read(cx).approvals.error.is_some()
                && pending(&view, cx).is_some_and(|id| id != first)
        });
        let second = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        assert!(!fixture.directory.path().join("project/second.txt").exists());
        tap(visual, &format!("live-approval-approve-{second}"));
        assert_eq!(lost.ids.lock().unwrap().len(), 1);
        tap(visual, "live-approval-retry");
        wait(visual, |cx| view.read(cx).approvals.attempt.is_none());
        let ids = lost.ids.lock().unwrap().clone();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(
            *lost.decisions.lock().unwrap(),
            vec![Decision::Approve, Decision::Approve]
        );
        assert!(!view.read_with(visual, |view, _| view.approvals.open));
        tap(visual, "live-approval-trigger");
        tap(visual, &format!("live-approval-reject-{second}"));
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).approvals.pending
        });
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            let approvals = &page.approvals;
            assert_eq!(approvals.len(), 2);
            assert_eq!(approvals[0].choice, Some(ApprovalChoice::Once));
            assert_eq!(approvals[1].state, ApprovalState::Denied);
            assert!(
                approvals
                    .iter()
                    .all(|approval| approval.source == ApprovalSource::User
                        && approval.access == Some(Access::Unrestricted)
                        && !approval.can_remember)
            );
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/first.txt")).unwrap(),
            "first"
        );
        assert!(!fixture.directory.path().join("project/second.txt").exists());
        let completed = restored(&fixture);
        projection.reconnect(2).unwrap();
        projection
            .apply(
                2,
                sailry_protocol::Update::ConversationSnapshot(completed.clone()),
            )
            .unwrap();
        let recovered = &projection.snapshot().unwrap().page.approvals;
        assert_eq!(recovered, &completed.page.approvals);
        assert_eq!(recovered.len(), 2);
        let original = &initial.page.approvals[0];
        let recovered = recovered
            .iter()
            .find(|approval| approval.id == original.id)
            .unwrap();
        assert_eq!(recovered.state, ApprovalState::Approved);
        assert_eq!(recovered.choice, Some(ApprovalChoice::Once));
        assert_eq!(recovered.source, original.source);
        assert_eq!(recovered.access, original.access);
        assert_eq!(recovered.can_remember, original.can_remember);
        assert_eq!(recovered.prefix, original.prefix);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[cfg(unix)]
#[gpui::test]
fn runs_unrestricted_automatically(cx: &mut TestAppContext) {
    use sailry_protocol::{Permission, conversation::ApprovalSource, process::Access};
    init(cx);
    for remote in [false, true] {
        let command =
            json!({"command":"printf 'approved\\n' >> ../outside.txt","access":"unrestricted"});
        let mut fixture = Fixture::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    command.clone(),
                ),
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    command,
                ),
            ],
        );
        let mut config = fixture.session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        }) else {
            panic!("session expected")
        };
        fixture.session = session;
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Run both commands in Full mode".into(),
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        assert!(view.read_with(visual, |_, cx| pending(&view, cx).is_none()));
        let approvals = view.read_with(visual, |view, _| {
            assert!(!view.approvals.pending);
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(page.approvals.len(), 2);
            assert!(
                page.approvals
                    .iter()
                    .all(|approval| approval.state == ApprovalState::Approved
                        && approval.source == ApprovalSource::Full
                        && approval.access == Some(Access::Unrestricted))
            );
            page.approvals.clone()
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("pending-approvals").is_none());
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("outside.txt")).unwrap(),
            "approved\napproved\n"
        );
        assert_eq!(restored(&fixture).page.approvals, approvals);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn stops_and_rejects_stale_callbacks(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).pending
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.resolve_approval(id, Decision::Approve, window, cx)
            })
        });
        assert!(view.read_with(visual, |view, _| view.approvals.attempt.is_none()));
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        assert!(!fixture.directory.path().join("project/资料.txt").exists());
        assert!(visual.debug_bounds("pending-approvals").is_none());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
