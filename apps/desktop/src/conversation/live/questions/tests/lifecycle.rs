use super::*;
use sailry_link::{Admission, Pending, Subscription as Stream, Transport};
use sailry_protocol::{ErrorCode, Fault, RequestId, Topic};
use std::sync::Mutex;

struct Lost {
    inner: Arc<dyn Transport>,
    ids: Mutex<Vec<RequestId>>,
    release: tokio::sync::Semaphore,
}

impl Transport for Lost {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let lose = if matches!(request.command, Command::ResolveQuestion { .. }) {
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
            self.release.acquire().await.unwrap().forget();
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "injected missing response",
            ))
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Stream>, Fault>> {
        self.inner.subscribe(topic)
    }
}

#[gpui::test]
fn retries_the_original_answer(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![text(false, 100), text(false, 100)]);
        let lost = Arc::new(Lost {
            inner: fixture.transport.clone(),
            ids: Mutex::new(Vec::new()),
            release: tokio::sync::Semaphore::new(0),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(lost.clone()));
        fixture.start();
        let (view, visual) = open(cx, binding, fixture.session.clone());
        let first = ready(&view, visual, None);
        tap(visual, "live-chat-input");
        visual.simulate_input("next draft");
        tap(visual, "live-question-trigger");
        tap(visual, "live-question-input");
        visual.simulate_input("exact answer 中文");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.page.questions.len() == 2)
        });
        assert!(view.read_with(visual, |view, _| view.questions.pending));
        tap(visual, "live-question-input");
        visual.simulate_input("must not overwrite a submitted answer");
        assert_eq!(
            view.read_with(visual, |view, cx| view
                .questions
                .editing
                .as_ref()
                .unwrap()
                .text(cx)),
            "exact answer 中文"
        );
        lost.release.add_permits(1);
        wait(visual, |cx| view.read(cx).questions.error.is_some());
        assert_eq!(view.read_with(visual, |view, _| current(view)), Some(first));
        tap(visual, "live-question-secondary");
        assert_eq!(lost.ids.lock().unwrap().len(), 1);
        tap(visual, "live-question-submit");
        let second = ready(&view, visual, Some(first));
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "next draft");
            assert!(view.questions.editing.as_ref().unwrap().text(cx).is_empty());
            assert!(view.questions.attempt.is_none());
            assert!(view.questions.error.is_none());
        });
        assert_ne!(second, first);
        let ids = lost.ids.lock().unwrap().clone();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1]);
        tap(visual, "live-question-secondary");
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).questions.pending
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
