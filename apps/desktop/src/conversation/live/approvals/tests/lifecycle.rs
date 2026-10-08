use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, RequestId, Topic};
use std::sync::Mutex;

struct Lost {
    inner: Arc<dyn Transport>,
    ids: Mutex<Vec<RequestId>>,
    decisions: Mutex<Vec<Decision>>,
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
        tap(visual, &format!("live-approval-reject-{next}"));
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).approvals.pending
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
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
