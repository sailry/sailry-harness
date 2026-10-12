use super::*;
use gpui_kit::test::TestWindowExt as _;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{Fault, Topic, Update};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

struct Failing {
    inner: Arc<dyn Transport>,
    fail: AtomicBool,
    disconnect: Mutex<Option<CancellationToken>>,
    subscriptions: AtomicUsize,
}

struct Interrupted {
    inner: Box<dyn Subscription>,
    disconnect: CancellationToken,
}

impl Subscription for Interrupted {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            tokio::select! {
                biased;
                _ = self.disconnect.cancelled() => Err(Fault::invalid("injected stream failure")),
                update = self.inner.next() => update,
            }
        })
    }
}

impl Transport for Failing {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        self.inner.dispatch(request)
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            let disconnect = if matches!(topic, Topic::Conversation(_)) {
                self.subscriptions.fetch_add(1, Ordering::SeqCst);
                if self.fail.load(Ordering::SeqCst) {
                    return Err(Fault::invalid("injected snapshot failure"));
                }
                self.disconnect.lock().unwrap().take()
            } else {
                None
            };
            let inner = self.inner.subscribe(topic).await?;
            let subscription: Box<dyn Subscription> = match disconnect {
                Some(disconnect) => Box::new(Interrupted { inner, disconnect }),
                None => inner,
            };
            Ok(subscription)
        })
    }
}

#[gpui::test]
fn failed_retry_preserves_history(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        fixture.start();
        let disconnect = CancellationToken::new();
        let transport = Arc::new(Failing {
            inner: fixture.transport.clone(),
            fail: AtomicBool::new(false),
            disconnect: Mutex::new(Some(disconnect.clone())),
            subscriptions: AtomicUsize::new(0),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(transport.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            let view = view.read(cx);
            view.connected()
                && view.history.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input("Keep this draft");
        let retained = view.read_with(visual, |view, _| view.history.clone());
        let turns = view.read_with(visual, |view, _| view.rows.clone());
        assert!(!turns.is_empty());
        let selector: &'static str = Box::leak(format!("live-turn-{}", turns[0]).into_boxed_str());
        visual.update(|window, cx| {
            window.render_frame(cx);
        });
        let before = visual.debug_bounds("live-chat-input").unwrap();
        let message = visual.debug_bounds(selector).unwrap();
        transport.fail.store(true, Ordering::SeqCst);
        disconnect.cancel();
        wait(visual, |cx| view.read(cx).history.error.is_some());
        let subscriptions = transport.subscriptions.load(Ordering::SeqCst);
        fixture::tap(visual, "chat-sync-retry");
        wait(visual, |cx| {
            transport.subscriptions.load(Ordering::SeqCst) > subscriptions
                && view.read(cx).history.error.is_some()
        });
        view.read_with(visual, |view, cx| {
            assert!(!view.connected());
            assert!(Arc::ptr_eq(
                view.history.snapshot.as_ref().unwrap(),
                retained.snapshot.as_ref().unwrap(),
            ));
            assert!(Arc::ptr_eq(&view.history.calls, &retained.calls));
            assert_eq!(view.history.artifact_cursor, retained.artifact_cursor);
            assert_eq!(view.rows, turns);
            assert_eq!(view.draft(cx), "Keep this draft");
        });
        visual.update(|window, cx| {
            window.render_frame(cx);
        });
        assert!(visual.debug_bounds("live-history-loading").is_none());
        assert_eq!(visual.debug_bounds("live-chat-input").unwrap(), before);
        assert_eq!(visual.debug_bounds(selector).unwrap(), message);
        transport.fail.store(false, Ordering::SeqCst);
        fixture::tap(visual, "chat-sync-retry");
        wait(visual, |cx| view.read(cx).connected());
        view.read_with(visual, |view, cx| {
            assert_eq!(view.rows, turns);
            assert_eq!(view.draft(cx), "Keep this draft");
        });
        visual.update(|window, cx| {
            window.render_frame(cx);
        });
        assert_eq!(visual.debug_bounds("live-chat-input").unwrap(), before);
        assert_eq!(visual.debug_bounds(selector).unwrap(), message);
        assert_eq!(
            fixture.task_requests(),
            1,
            "retry must not replay the Agent turn"
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
