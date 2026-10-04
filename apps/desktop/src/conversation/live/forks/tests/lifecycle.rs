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
            if !matches!(request.command, Command::ForkConversation { .. }) {
                return self.inner.dispatch(request).await;
            }
            self.requests.lock().unwrap().push(request.id);
            let admission = self.inner.dispatch(request).await?;
            if self.lose.swap(false, Ordering::SeqCst) {
                admission.completion.await.unwrap().unwrap();
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected fork response loss",
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
fn retries_bound_request(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let turn = complete(&fixture);
        let observed = Arc::new(Observed {
            inner: fixture.transport.clone(),
            lose: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let created = observe(&view, visual);
        choose(&view, turn, visual);
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).error.is_some()
        });
        assert!(created.lock().unwrap().is_none());
        tap(visual, "live-chat-input");
        visual.simulate_input("Typing after uncertain result 中文 🙂");
        visual.update(|window, cx| view.update(cx, |view, cx| view.fork(turn, window, cx)));
        assert_eq!(observed.requests.lock().unwrap().len(), 1);
        visual.update(|window, cx| view.update(cx, |view, cx| view.retry(window, cx)));
        wait(visual, |_| created.lock().unwrap().is_some());
        let requests = observed.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        drop(requests);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 2);
        let latest = fixture.session.revision;
        let mut config = fixture.session.config.clone();
        config.effort = Effort::Low;
        fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: latest,
            config,
        });
        wait(visual, |cx| {
            view.read(cx).session.as_ref().unwrap().revision == latest + 1
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.session.as_mut().unwrap().revision = latest;
                view.fork(turn, window, cx);
            })
        });
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).error == Some("chat_config_changed")
        });
        assert!(view.read_with(visual, |view, _| view.retry.is_none()));
        assert_eq!(observed.requests.lock().unwrap().len(), 3);
        // A menu callback must revalidate the current row and connection before sending.
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.fork(TurnId::new(), window, cx);
                view.history.connected = false;
                view.fork(turn, window, cx);
            })
        });
        assert_eq!(observed.requests.lock().unwrap().len(), 3);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
            "Typing after uncertain result 中文 🙂"
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        fixture.close();
    }
}
