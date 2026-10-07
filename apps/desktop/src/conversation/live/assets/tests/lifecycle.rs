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
    queries: Mutex<Vec<Option<Kind>>>,
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
            let Command::ListConversationAssets { query, .. } = &request.command else {
                return self.inner.dispatch(request).await;
            };
            self.queries.lock().unwrap().push(query.kind);
            if query.kind.is_none() && self.fail.swap(false, Ordering::SeqCst) {
                return Err(Fault::new(ErrorCode::Unavailable, "injected asset failure"));
            }
            let hold = query.kind.is_none() && self.hold.swap(false, Ordering::SeqCst);
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

#[gpui::test]
fn retries_and_cancels_superseded_queries(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::markdown(
                "[Result](result.md)".into(),
            ))
        });
        submit(&fixture, "[Input](input.txt)".into());
        let observed = Arc::new(Observed::new(fixture.transport.clone()));
        fixture.binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        tap(visual, "live-assets");
        wait(visual, |cx| panel.read(cx).failed);
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("chat_assets_failed")
        );
        let initial_queries = observed.queries.lock().unwrap().len();
        let initial_retry = visual.debug_bounds("asset-retry");
        tap(visual, "asset-retry");
        visible_retry(visual);
        wait(visual, |cx| {
            observed.queries.lock().unwrap().len() == initial_queries + 1 && !panel.read(cx).loading
        });
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.groups.len()),
            2,
            "initial asset Retry failed: remote={remote}, queries={initial_queries}->{}, retry={initial_retry:?}->{:?}, notice={:?}",
            observed.queries.lock().unwrap().len(),
            visual.debug_bounds("asset-retry"),
            visual.debug_bounds("notification-card")
        );
        let previous = panel.read_with(visual, |panel, _| {
            (panel.groups.clone(), panel.before, panel.loaded_revision)
        });
        observed.fail.store(true, Ordering::SeqCst);
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(visual, "asset-refresh");
        wait(visual, |cx| panel.read(cx).failed);
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("chat_assets_failed")
        );
        assert_eq!(
            panel.read_with(visual, |panel, _| {
                (panel.groups.clone(), panel.before, panel.loaded_revision)
            }),
            previous
        );
        let queries = observed.queries.lock().unwrap().len();
        let retry = visual.debug_bounds("asset-retry");
        tap(visual, "asset-retry");
        visible_retry(visual);
        wait(visual, |cx| {
            observed.queries.lock().unwrap().len() == queries + 1 && !panel.read(cx).loading
        });
        assert_eq!(panel.read_with(visual, |panel, _| panel.groups.len()), 2);
        let state = panel.read_with(visual, |panel, _| (panel.failed, panel.loading, panel.open));
        let rendered = visual.debug_bounds("asset-retry");
        let content = visual.debug_bounds("asset-panel");
        let notice = visual.debug_bounds("notification-card");
        let viewport = visual.update(|window, _| window.viewport_size());
        assert!(
            !state.0,
            "asset Retry failed: remote={remote}, state={state:?}, queries={queries}->{}, retry={retry:?}->{rendered:?}, content={content:?}, notice={notice:?}, viewport={viewport:?}",
            observed.queries.lock().unwrap().len()
        );
        observed.hold.store(true, Ordering::SeqCst);
        tap(visual, "asset-refresh");
        fixture.runtime.block_on(observed.wait_held());
        panel.update_in(visual, |panel, window, cx| panel.load(false, window, cx));
        wait(visual, |cx| !panel.read(cx).loading);
        wait(visual, |_| observed.cancelled.load(Ordering::SeqCst) == 1);
        assert_eq!(panel.read_with(visual, |panel, _| panel.groups.len()), 2);
        for (outside, cancelled) in [(false, 2), (true, 3)] {
            observed.hold.store(true, Ordering::SeqCst);
            tap(visual, "asset-refresh");
            fixture.runtime.block_on(observed.wait_held());
            if outside {
                let position = visual
                    .update(|window, _| point(px(1.), window.viewport_size().height - px(1.)));
                assert!(
                    !visual
                        .debug_bounds("asset-panel")
                        .unwrap()
                        .contains(&position)
                );
                visual.simulate_click(position, Modifiers::default());
            } else {
                visual.simulate_keystrokes("escape");
            }
            wait(visual, |cx| !panel.read(cx).open);
            wait(visual, |_| {
                observed.cancelled.load(Ordering::SeqCst) == cancelled
            });
            assert!(visual.debug_bounds("asset-panel").is_none());
            tap(visual, "live-assets");
            wait(visual, |cx| panel.read(cx).open && !panel.read(cx).loading);
            assert_eq!(panel.read_with(visual, |panel, _| panel.groups.len()), 2);
        }
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| !panel.read(cx).open);
        assert!(observed.queries.lock().unwrap().iter().all(Option::is_none));
        assert_eq!(fixture.task_requests(), 1);
        fixture.close();
    }
}
