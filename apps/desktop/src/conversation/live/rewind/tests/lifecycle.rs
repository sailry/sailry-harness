use super::*;
use sailry_link::{Admission, Pending, Subscription as Stream, Transport};
use sailry_protocol::{ErrorCode, Fault, RequestId, Topic};
use std::sync::atomic::{AtomicBool, Ordering};

struct Observed {
    inner: Arc<dyn Transport>,
    lose: AtomicBool,
    requests: Mutex<Vec<RequestId>>,
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if !matches!(request.command, Command::RewindConversation { .. }) {
                return self.inner.dispatch(request).await;
            }
            self.requests.lock().unwrap().push(request.id);
            let admission = self.inner.dispatch(request).await?;
            if self.lose.swap(false, Ordering::SeqCst) {
                admission.completion.await.unwrap().unwrap();
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected rewind response loss",
                ));
            }
            Ok(admission)
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Stream>, Fault>> {
        self.inner.subscribe(topic)
    }
}

#[gpui::test]
fn retries_confirmed_request(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let first = complete(&fixture, "Retained");
        complete(&fixture, "Removed");
        let observed = Arc::new(Observed {
            inner: fixture.transport.clone(),
            lose: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        choose(&view, first, visual);
        let newer = complete(&fixture, "Arrived during confirmation");
        wait(visual, |cx| view.read(cx).rows.contains(&newer));
        crate::prompts::tests::answer(visual, "chat_rewind_action");
        assert_eq!(
            view.read_with(visual, |view, _| view.error),
            Some("chat_history_changed")
        );
        assert!(observed.requests.lock().unwrap().is_empty());
        choose(&view, first, visual);
        crate::prompts::tests::answer(visual, "chat_rewind_action");
        wait(visual, |cx| {
            !view.read(cx).pending
                && view.read(cx).error == Some("chat_action_unknown")
                && view
                    .read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .revision
                    == 2
        });
        assert!(view.read_with(visual, |view, _| view.backup.is_none()));
        tap(visual, "live-chat-input");
        visual.simulate_input("New typing after lost result 🙂");
        visual.update(|window, cx| view.update(cx, |view, cx| view.retry(window, cx)));
        wait(visual, |cx| view.read(cx).backup.is_some());
        let requests = observed.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        drop(requests);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
            "New typing after lost result 🙂"
        );
        let last = complete(&fixture, "A later turn");
        wait(visual, |cx| view.read(cx).rows.contains(&last));
        let Output::QueuedTurn(queued) = fixture.execute(Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: 1,
            message: "Keep pending".into(),
        }) else {
            panic!("queued turn expected")
        };
        wait(visual, |cx| {
            !view
                .read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .queue
                .items
                .is_empty()
        });
        assert!(view.read_with(visual, |view, _| view.rewind_request(first).is_none()));
        fixture.execute(Command::StopTurn { turn: queued.id });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .queue
                .items
                .is_empty()
        });
        view.update(visual, |view, _| {
            view.history.connected = false;
        });
        assert!(view.read_with(visual, |view, _| view.rewind_request(first).is_none()));
        assert_eq!(observed.requests.lock().unwrap().len(), 2);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 4);
        fixture.close();
    }
}
