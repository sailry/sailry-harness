use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, RequestId, Topic};
use std::sync::Mutex;

struct Lost {
    inner: Arc<dyn Transport>,
    ids: Mutex<Vec<RequestId>>,
}

impl Transport for Lost {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let lose = if matches!(request.command, Command::ResolveApproval { .. }) {
                let mut ids = self.ids.lock().unwrap();
                ids.push(request.id);
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
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.start();
        let lost = Arc::new(Lost {
            inner: fixture.transport.clone(),
            ids: Mutex::new(Vec::new()),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(lost.clone()));
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        tap(visual, &format!("live-approval-approve-{id}"));
        wait(visual, |cx| {
            view.read(cx).approvals.error.is_some()
                && pending(&view, cx).is_some_and(|next| next != id)
        });
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
