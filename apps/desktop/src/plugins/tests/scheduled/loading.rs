use super::*;
use sailry_link::{Admission, CancellationToken, Pending, Subscription, Transport};
use sailry_protocol::{Fault, NodeId, Request, StreamId, Topic};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(in crate::plugins::tests) struct Reads {
    inner: Arc<dyn Transport>,
    name: &'static str,
    pause: Mutex<Option<(CancellationToken, CancellationToken)>>,
    failure: AtomicBool,
}

impl Reads {
    pub(in crate::plugins::tests) fn bind(fixture: &mut Fixture, name: &'static str) -> Arc<Self> {
        let reads = Arc::new(Self {
            inner: fixture.transport.clone(),
            name,
            pause: Mutex::new(None),
            failure: AtomicBool::new(false),
        });
        fixture.binding.client = Arc::new(sailry_client::Client::new(reads.clone()));
        fixture.binding.defaults = fixture.binding.client.clone();
        reads
    }

    pub(in crate::plugins::tests) fn hold(&self) -> (CancellationToken, CancellationToken) {
        let entered = CancellationToken::new();
        let release = CancellationToken::new();
        *self.pause.lock().unwrap() = Some((entered.clone(), release.clone()));
        (entered, release)
    }

    pub(in crate::plugins::tests) fn fail(&self) {
        self.failure.store(true, Ordering::SeqCst);
    }
}

impl Transport for Reads {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn open(&self, stream: StreamId) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        self.inner.open(stream)
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::ListPluginKeys { .. })
                && request
                    .plugin
                    .as_ref()
                    .is_some_and(|scope| scope.package.name == self.name)
            {
                let pause = self.pause.lock().unwrap().take();
                if let Some((entered, release)) = pause {
                    entered.cancel();
                    release.cancelled().await;
                }
                if self.failure.swap(false, Ordering::SeqCst) {
                    return Err(Fault::new(
                        sailry_protocol::ErrorCode::Unavailable,
                        "Injected plugin list read failure",
                    ));
                }
            }
            self.inner.dispatch(request).await
        })
    }
}

fn geometry(visual: &mut VisualTestContext) -> Vec<(Point<Pixels>, Pixels)> {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    ["tasks-content", "tasks-heading", "tasks-toolbar"]
        .map(|selector| {
            let bounds = visual.debug_bounds(selector).unwrap();
            (bounds.origin, bounds.size.width)
        })
        .to_vec()
}

fn task(fixture: &Fixture, package: &plugin::Info) -> Value {
    call(
        fixture,
        package,
        "save",
        json!({"revision":"0","name":"Retained task","prompt":"Do not run","queue":"default","project":null,"worktree":null,"config":fixture.session.config,"enabled":false,"timing":{"kind":"once","data":{"at_ms":chrono::Utc::now().timestamp_millis()+600000}}}),
    )
}

#[gpui::test]
fn keeps_geometry_until_records_arrive(cx: &mut TestAppContext) {
    init(cx);
    for (remote, populated) in [false, true]
        .into_iter()
        .flat_map(|remote| [false, true].map(move |populated| (remote, populated)))
    {
        let mut fixture = Fixture::new(remote);
        let package = install(&fixture);
        let saved = populated.then(|| task(&fixture, &package));
        let reads = Reads::bind(&mut fixture, "scheduled-tasks");
        let (entered, release) = reads.hold();
        let (panel, visual) = mount(&fixture, &package, cx);
        wait(visual, |_| entered.is_cancelled());
        let pending = geometry(visual);
        let row = visual.debug_bounds("tasks-items-loading").unwrap();
        assert!(
            visual
                .debug_bounds("empty-card-tasks-items-empty")
                .is_none()
        );
        assert!(
            visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("(\"loaded\", Bool(false))")
        );
        click(visual, "tasks-tab-runs");
        assert_eq!(pending, geometry(visual));
        assert!(visual.debug_bounds("tasks-runs-loading").is_some());
        assert!(visual.debug_bounds("empty-card-tasks-runs-empty").is_none());
        click(visual, "tasks-tab-tasks");
        release.cancel();
        let selector = saved.as_ref().map_or("empty-tasks-items-empty", |saved| {
            select(format!("task-card-{}", saved["id"].as_str().unwrap()))
        });
        wait(visual, |cx| {
            list_ready(&panel, "tasks-items", cx)
                && (!populated || snapshot(&panel, cx).contains(selector))
        });
        assert_eq!(pending, geometry(visual));
        assert!(visual.debug_bounds("tasks-items-loading").is_none());
        if populated {
            assert_eq!(row, visual.debug_bounds(selector).unwrap());
            assert!(
                visual
                    .debug_bounds("empty-card-tasks-items-empty")
                    .is_none()
            );
        } else {
            assert!(
                visual
                    .debug_bounds("empty-card-tasks-items-empty")
                    .is_some()
            );
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn failed_refresh_retains_cards_and_editor(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let package = install(&fixture);
        let saved = task(&fixture, &package);
        let reads = Reads::bind(&mut fixture, "scheduled-tasks");
        let (panel, visual) = mount(&fixture, &package, cx);
        let id = saved["id"].as_str().unwrap();
        let card = select(format!("task-card-{id}"));
        wait(visual, |cx| snapshot(&panel, cx).contains(card));
        click(visual, select(format!("task-edit-{id}")));
        wait(visual, |cx| snapshot(&panel, cx).contains("task-editor"));
        let before = geometry(visual);
        let card_bounds = visual.debug_bounds(card).unwrap();
        let (entered, release) = reads.hold();
        reads.fail();
        let mut changed = saved;
        changed["name"] = "Current Node task".into();
        call(&fixture, &package, "save", changed);
        wait(visual, |_| entered.is_cancelled());
        assert_eq!(before, geometry(visual));
        assert_eq!(card_bounds, visual.debug_bounds(card).unwrap());
        assert!(
            visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("task-editor")
        );
        assert!(visual.debug_bounds("tasks-items-loading").is_none());
        release.cancel();
        toast(visual, "Could not load tasks");
        assert_eq!(before, geometry(visual));
        assert_eq!(card_bounds, visual.debug_bounds(card).unwrap());
        assert!(
            visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("task-editor")
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
