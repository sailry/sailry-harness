use super::super::*;
use crate::discovery_fixture::{Reply, Server};
use sailry_client::Client;
use sailry_link::{Admission, Link, NetworkScope, Pending, Subscription, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Fault, Output, Request, Topic, conversation::ModelApi};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(in crate::settings::search) struct Observed {
    inner: Arc<dyn Transport>,
    pub requests: Mutex<Vec<Request>>,
    pub lose_reply: AtomicBool,
    lose_inspect: AtomicBool,
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::InspectRequest { .. })
                && self.lose_inspect.swap(false, Ordering::SeqCst)
            {
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::Unavailable,
                    "fixture lost inspection reply",
                ));
            }
            let tracked = matches!(
                request.command,
                Command::SaveSearchSettings(_)
                    | Command::TestSearchDriver { .. }
                    | Command::SetPluginEnabled { .. }
            );
            if tracked {
                self.requests.lock().unwrap().push(request.clone());
            }
            let dropped = tracked && self.lose_reply.swap(false, Ordering::SeqCst);
            let mut admission = self.inner.dispatch(request).await?;
            if dropped {
                self.lose_inspect.store(true, Ordering::SeqCst);
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let completion = std::mem::replace(&mut admission.completion, receiver);
                tokio::spawn(async move {
                    _ = completion.await;
                    drop(sender);
                });
            }
            Ok(admission)
        })
    }
}

pub(in crate::settings::search) struct Fixture {
    directory: tempfile::TempDir,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub node: Node,
    pub other: Node,
    controller: Link,
    pub transport: Arc<Observed>,
    pub server: Server,
}

impl Fixture {
    pub fn new(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(Node::start_isolated(directory.path().join("node")))
            .unwrap();
        let other = runtime
            .block_on(Node::start_isolated(directory.path().join("other")))
            .unwrap();
        let controller = runtime
            .block_on(Link::controller(
                directory.path().join("controller"),
                NetworkScope::default(),
            ))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .handle()
                    .pair(node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let inner = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let transport = Arc::new(Observed {
            inner,
            requests: Mutex::default(),
            lose_reply: AtomicBool::new(false),
            lose_inspect: AtomicBool::new(false),
        });
        let server = runtime.block_on(Server::start_with_request(ModelApi::Anthropic, |_| Reply::Json(serde_json::json!({"results":[{"title":"Source","url":"https://example.com/source","text":"Result","content":"Result"}]}))));
        node.set_search_fixture(Driver::Exa, &server.endpoint);
        node.set_search_fixture(Driver::Searxng, &server.endpoint);
        Self {
            directory,
            runtime,
            node,
            other,
            controller,
            transport,
            server,
        }
    }

    pub fn execute(&self, command: Command) -> Output {
        let client = Client::new(self.node.local());
        self.runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    }

    pub fn settings(&self) -> Settings {
        let Output::SearchSettings(settings) = self.execute(Command::ReadSearchSettings) else {
            panic!("search settings expected")
        };
        settings
    }

    pub fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(crate::preferences::Preferences::open(
                self.directory.path().join("preferences.json"),
            ))
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    self.transport.clone(),
                    self.runtime.clone(),
                    "Fixture host".into(),
                    cx,
                );
                workspace.open_plugin_settings("web-search", cx);
            });
            owner = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let owner = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(960.), px(1100.)));
        wait(visual, |cx| {
            selected(&owner, cx)
                .is_some_and(|panel| panel.read(cx).saved.is_some() && !panel.read(cx).pending)
                && owner.read(cx).provider_link.as_ref().unwrap().connected
                && !owner.read(cx).plugin_catalog.busy()
        });
        (owner, visual)
    }

    pub fn bind(&self, owner: &Entity<Workspace>, visual: &mut VisualTestContext, other: bool) {
        let transport: Arc<dyn Transport> = if other {
            self.other.local()
        } else {
            self.transport.clone()
        };
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    transport,
                    self.runtime.clone(),
                    "Selected host".into(),
                    cx,
                )
            })
        });
        wait(visual, |cx| {
            selected(owner, cx)
                .is_some_and(|panel| panel.read(cx).saved.is_some() && !panel.read(cx).pending)
        });
    }

    pub fn close(self, visual: &mut VisualTestContext) {
        visual.update(|window, _| window.remove_window());
        self.runtime.block_on(self.node.shutdown()).unwrap();
        self.runtime.block_on(self.other.shutdown()).unwrap();
        self.runtime.block_on(self.controller.close()).unwrap();
    }
}

pub(in crate::settings::search) fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("en");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
}

pub(in crate::settings::search) fn selected(
    owner: &Entity<Workspace>,
    cx: &App,
) -> Option<Entity<Panel>> {
    let workspace = owner.read(cx);
    let node = workspace.provider_link.as_ref()?.binding.client.target();
    workspace.search.panels.get(&node).cloned()
}

pub(in crate::settings::search) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

pub(in crate::settings::search) fn wait(cx: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        if cx.update(|_, cx| ready(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "search settings UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(in crate::settings::search) fn tap(cx: &mut VisualTestContext, selector: &'static str) {
    if cx.update(|window, cx| window.has_active_dialog(cx)) {
        std::thread::sleep(*dialog::ANIMATION_DURATION);
    }
    draw(cx);
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

pub(in crate::settings::search) fn input(
    cx: &mut VisualTestContext,
    selector: &'static str,
    value: &str,
) {
    tap(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(value);
    draw(cx);
}

pub(in crate::settings::search) fn toast(cx: &mut VisualTestContext, key: &str) {
    let expected = crate::tr(key);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        if cx.update(|window, cx| {
            !window.notifications(cx).is_empty()
                && crate::feedback::tests::summary(window, cx).contains(expected.as_ref())
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "expected search toast: {key}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
