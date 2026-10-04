use super::*;
use sailry_link::{Admission, Pending, Subscription as Stream, Transport};
use sailry_protocol::{ErrorCode, Fault, NodeId, Request, Topic};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;

struct Observed {
    inner: Arc<dyn Transport>,
    fail: AtomicBool,
    hold: AtomicBool,
    queries: Mutex<Vec<String>>,
    held: Semaphore,
    release: Semaphore,
    cancelled: Arc<AtomicUsize>,
}

impl Observed {
    fn new(inner: Arc<dyn Transport>) -> Self {
        Self {
            inner,
            fail: AtomicBool::new(true),
            hold: AtomicBool::new(false),
            queries: Mutex::new(Vec::new()),
            held: Semaphore::new(0),
            release: Semaphore::new(0),
            cancelled: Arc::new(AtomicUsize::new(0)),
        }
    }

    async fn wait_held(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.held.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}

struct Held(Arc<AtomicUsize>, bool);
impl Drop for Held {
    fn drop(&mut self) {
        if !self.1 {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let Command::SearchConversation { query, .. } = &request.command else {
                return self.inner.dispatch(request).await;
            };
            self.queries.lock().unwrap().push(query.text.clone());
            if query.text == "History" && self.fail.swap(false, Ordering::SeqCst) {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected search failure",
                ));
            }
            let hold = query.text == "History" && self.hold.swap(false, Ordering::SeqCst);
            let mut admission = self.inner.dispatch(request).await?;
            if hold {
                let result = admission.completion.await.unwrap();
                let mut held = Held(self.cancelled.clone(), false);
                self.held.add_permits(1);
                self.release.acquire().await.unwrap().forget();
                held.1 = true;
                let (sender, receiver) = tokio::sync::oneshot::channel();
                sender.send(result).unwrap();
                admission.completion = receiver;
            }
            Ok(admission)
        })
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Stream>, Fault>> {
        self.inner.subscribe(topic)
    }
}

pub(super) fn query(view: &Entity<View>, cx: &mut VisualTestContext, text: &str) {
    cx.update(|window, cx| {
        view.read(cx)
            .search
            .read(cx)
            .list
            .clone()
            .update(cx, |list, cx| list.focus(window, cx))
    });
    cx.simulate_keystrokes("secondary-a backspace");
    cx.simulate_input(text);
}

#[gpui::test]
fn query_lifecycle(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        for index in 0..25 {
            submit(&fixture, format!("History {index} 中文 🙂"));
        }
        let observed = Arc::new(Observed::new(fixture.transport.clone()));
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Draft while searching 🙂");
        tap(visual, "live-search");
        visual.simulate_input("History");
        wait(visual, |cx| {
            !view
                .read(cx)
                .search
                .read(cx)
                .list
                .read(cx)
                .delegate()
                .loading
        });
        assert!(
            view.read_with(visual, |view, cx| view
                .search
                .read(cx)
                .list
                .read(cx)
                .delegate()
                .error
                .is_some()),
            "search requests: {:?}",
            observed.queries.lock().unwrap()
        );
        tap(visual, "live-search-retry");
        searched(&view, visual, 20);
        visual.simulate_keystrokes(&vec!["down"; 19].join(" "));
        searched(&view, visual, 25);
        assert!(visual.debug_bounds("live-search-more").is_none());
        observed.hold.store(true, Ordering::SeqCst);
        query(&view, visual, "History");
        fixture.runtime.block_on(observed.wait_held());
        query(&view, visual, "No match");
        searched(&view, visual, 0);
        wait(visual, |_| observed.cancelled.load(Ordering::SeqCst) == 1);
        assert_eq!(
            view.read_with(visual, |view, cx| view
                .search
                .read(cx)
                .list
                .read(cx)
                .delegate()
                .query
                .clone()),
            "No match"
        );

        let count = observed.queries.lock().unwrap().len();
        // Submit one over-limit value; character-by-character typing can legitimately
        // send shorter intermediate queries when the test runner is busy.
        visual.update(|window, cx| {
            view.read(cx)
                .search
                .read(cx)
                .list
                .clone()
                .update(cx, |list, cx| list.set_query(&"🙂".repeat(129), window, cx));
        });
        wait(visual, |cx| {
            view.read(cx).search.read(cx).list.read(cx).delegate().error
                == Some("chat_search_limit")
        });
        assert_eq!(observed.queries.lock().unwrap().len(), count);
        assert!(visual.debug_bounds("live-search-more").is_none());

        observed.hold.store(true, Ordering::SeqCst);
        query(&view, visual, "History");
        fixture.runtime.block_on(observed.wait_held());
        visual.simulate_keystrokes("escape");
        wait(visual, |_| observed.cancelled.load(Ordering::SeqCst) == 2);
        assert!(!view.read_with(visual, |view, cx| view.search.read(cx).open));
        assert!(
            visual.update(|window, cx| view.read(cx).input.focus_handle(cx).is_focused(window))
        );
        tap(visual, "live-search");
        searched(&view, visual, 20);
        visual.simulate_keystrokes("down enter");
        wait(visual, |cx| !view.read(cx).search.read(cx).open);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
            "Draft while searching 🙂"
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 25);
        fixture.close();
    }
}
