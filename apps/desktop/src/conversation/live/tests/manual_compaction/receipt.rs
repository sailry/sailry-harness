use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, Topic};
use std::sync::Mutex;

struct Lost {
    inner: Arc<dyn Transport>,
    requests: Mutex<Vec<Request>>,
}

impl Transport for Lost {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let lose = if matches!(request.command, Command::CompactContext { .. }) {
                let mut requests = self.requests.lock().unwrap();
                requests.push(request.clone());
                requests.len() == 1
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
                "injected missing compaction response",
            ))
        })
    }
}

#[gpui::test]
fn retries_original_admission(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::compaction(false))
        });
        std::fs::write(
            fixture.directory.path().join("project/source.txt"),
            "Retained evidence",
        )
        .unwrap();
        let observed = Arc::new(Lost {
            inner: fixture.transport.clone(),
            requests: Mutex::new(vec![]),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input(&"Original constraint ".repeat(100));
        click(visual, "live-chat-send");
        finished(&view, visual, 1);
        click(visual, "live-chat-input");
        visual.simulate_input("Keep draft");
        request(&view, visual);
        finished(&view, visual, 2);
        wait(visual, |cx| view.read(cx).error.is_some());
        assert!(!view.read_with(visual, |view, _| view.can_compact()));
        click(visual, "live-chat-input");
        visual.simulate_input(" after missing response 中文 🙂");
        click(visual, "chat-retry");
        wait(visual, |cx| {
            view.read(cx).retry.is_none() && !view.read(cx).pending
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(
                view.input.read(cx).value(),
                "Keep draft after missing response 中文 🙂"
            );
            assert_eq!(view.history.snapshot.as_ref().unwrap().page.runs.len(), 2);
        });
        let requests = observed.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
