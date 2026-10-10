use super::*;
use sailry_link::{Admission, Link, NetworkScope, Pending, Subscription, Transport};
use sailry_node_runtime::{EnvironmentFixture, Node};
use sailry_protocol::{Command, Fault, NodeId, Output, Request, Topic, WorktreeId};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) struct Observed {
    inner: Arc<dyn Transport>,
    pub requests: Mutex<Vec<Request>>,
    pub drop_next: AtomicBool,
    drop_inspect: AtomicBool,
    pub hold: AtomicBool,
    pub release: Arc<tokio::sync::Semaphore>,
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
                && self.drop_inspect.swap(false, Ordering::SeqCst)
            {
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::Unavailable,
                    "fixture lost outcome inspection",
                ));
            }
            let tracked = matches!(
                request.command,
                Command::SaveCodeSettings { .. }
                    | Command::SetPluginEnabled { .. }
                    | Command::InstallCodeServer { .. }
                    | Command::CancelCodeInstall { .. }
                    | Command::UninstallCodeServer { .. }
                    | Command::StartCodeServer { .. }
                    | Command::StopCodeServer { .. }
                    | Command::UpdateCodeRules
            );
            if tracked {
                self.requests.lock().unwrap().push(request.clone());
            }
            let dropped = tracked && self.drop_next.swap(false, Ordering::SeqCst);
            if dropped {
                self.drop_inspect.store(true, Ordering::SeqCst);
            }
            let held = tracked && self.hold.load(Ordering::SeqCst);
            let mut admission = self.inner.dispatch(request).await?;
            if held || dropped {
                let release = self.release.clone();
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let completion = std::mem::replace(&mut admission.completion, receiver);
                tokio::spawn(async move {
                    let result = completion.await;
                    if held {
                        release.acquire().await.unwrap().forget();
                    }
                    if !dropped && let Ok(result) = result {
                        _ = sender.send(result);
                    }
                });
            }
            Ok(admission)
        })
    }
}

pub(super) struct Fixture {
    _directory: tempfile::TempDir,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub node: Node,
    pub other: Node,
    pub controller: Link,
    pub transport: Arc<Observed>,
    pub root: PathBuf,
    pub worktree: WorktreeId,
    pub second: WorktreeId,
}

impl Fixture {
    pub fn new(remote: bool) -> Self {
        Self::with_endpoint(remote, None)
    }

    pub fn with_endpoint(remote: bool, endpoint: Option<String>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let root = directory.path().canonicalize().unwrap().join("project");
        let bin = directory.path().canonicalize().unwrap().join("bin");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let environment = EnvironmentFixture {
            home: directory.path().into(),
            shell: "/bin/false".into(),
            environment: BTreeMap::from([("PATH".into(), bin.as_os_str().to_owned())]),
            timeout: Duration::from_secs(1),
        };
        let node = runtime
            .block_on(async {
                match endpoint {
                    Some(endpoint) => {
                        Node::start_with_code(directory.path().join("node"), environment, &endpoint)
                            .await
                    }
                    None => {
                        Node::start_with_environment(directory.path().join("node"), environment)
                            .await
                    }
                }
            })
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
            requests: Mutex::new(Vec::new()),
            drop_next: AtomicBool::new(false),
            drop_inspect: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            release: Arc::new(tokio::sync::Semaphore::new(0)),
        });
        let client = sailry_client::Client::new(transport.clone());
        runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Code fixture".into(),
                path: root.to_str().unwrap().into(),
            })))
            .unwrap();
        runtime.block_on(crate::settings::tests::packages::install(
            &client,
            directory.path(),
            "code",
        ));
        runtime.block_on(crate::settings::tests::packages::install(
            &sailry_client::Client::new(other.local()),
            &directory.path().join("other-fixtures"),
            "code",
        ));
        let second_root = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("other-project");
        std::fs::create_dir(&second_root).unwrap();
        std::fs::write(second_root.join("notes.txt"), "notes\n").unwrap();
        runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Notes fixture".into(),
                path: second_root.to_str().unwrap().into(),
            })))
            .unwrap();
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.path == root.to_str().unwrap())
            .unwrap()
            .id;
        let second = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.path == second_root.to_str().unwrap())
            .unwrap()
            .id;
        Self {
            _directory: directory,
            runtime,
            node,
            other,
            controller,
            transport,
            root,
            worktree,
            second,
        }
    }

    pub fn execute(&self, command: Command) -> Output {
        let client = sailry_client::Client::new(self.transport.clone());
        self.runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    }

    pub fn settings(&self) -> code::Settings {
        let Output::CodeSettings(settings) = self.execute(Command::ReadCodeSettings) else {
            panic!("settings expected")
        };
        settings
    }

    pub fn executable(&self, server: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.root.parent().unwrap().join("bin").join(server);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    pub fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let settings = cx.new(|cx| Workspace::new(window, cx));
            settings.update(cx, |settings, cx| {
                settings.bind_providers(
                    self.transport.clone(),
                    self.runtime.clone(),
                    "Fixture Node".into(),
                    cx,
                );
                settings.open_plugin_settings("code", cx);
            });
            owner = Some(settings.clone());
            Root::new(settings, window, cx)
        });
        let owner = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1100.), px(1100.)));
        wait(visual, |cx| bound(&owner, cx, self.transport.target()));
        (owner, visual)
    }

    pub fn bind(
        &self,
        owner: &Entity<Workspace>,
        cx: &mut VisualTestContext,
        other: bool,
        _tree: Option<WorktreeId>,
    ) {
        let transport: Arc<dyn Transport> = if other {
            self.other.local()
        } else {
            self.transport.clone()
        };
        let node = transport.target();
        cx.update(|_, cx| {
            owner.update(cx, |settings, cx| {
                settings.bind_providers(
                    transport.clone(),
                    self.runtime.clone(),
                    "Selected Node".into(),
                    cx,
                );
            })
        });
        wait(cx, |cx| bound(owner, cx, node));
        draw(cx);
    }

    pub fn close(self, cx: &mut VisualTestContext) {
        cx.update(|window, _| window.remove_window());
        self.runtime.block_on(self.node.shutdown()).unwrap();
        self.runtime.block_on(self.other.shutdown()).unwrap();
        self.runtime.block_on(self.controller.close()).unwrap();
    }
}

pub(super) fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("en");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
}
pub(super) fn selected(owner: &Entity<Workspace>, cx: &App) -> Entity<crate::plugins::Panel> {
    owner.read(cx).plugin_settings_panel().unwrap()
}
fn bound(owner: &Entity<Workspace>, cx: &App, node: NodeId) -> bool {
    owner
        .read(cx)
        .provider_link
        .as_ref()
        .is_some_and(|live| live.connected && live.binding.client.target() == node)
        && ready(owner, cx, "code-update-rules")
}

pub(super) fn ready(owner: &Entity<Workspace>, cx: &App, selector: &str) -> bool {
    owner.read(cx).plugin_settings_panel().is_some_and(|panel| {
        crate::plugins::diagnostics(&panel, cx).lines().any(|line| {
            line.contains(selector)
                && (line.contains("(\"disabled\", Bool(false))")
                    || line.contains(":disabled[Bool(false)]"))
        })
    })
}
#[track_caller]
pub(super) fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        draw(cx);
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "code settings deadline");
        cx.executor().advance_clock(Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
}
pub(super) fn lookup(cx: &mut VisualTestContext, selector: &str) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}
pub(super) fn tap(cx: &mut VisualTestContext, selector: &str) {
    std::thread::sleep(*dialog::ANIMATION_DURATION);
    let deadline = Instant::now() + Duration::from_secs(10);
    let bounds = loop {
        draw(cx);
        reveal(cx, selector);
        if let Some(bounds) = lookup(cx, selector) {
            break bounds;
        }
        assert!(Instant::now() < deadline, "missing {selector}");
        cx.executor().advance_clock(Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(10));
    };
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}
pub(super) fn input(cx: &mut VisualTestContext, selector: &str, value: &str) {
    tap(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(value);
    draw(cx);
}

pub(super) fn reveal(cx: &mut VisualTestContext, selector: &str) {
    if cx.update(|window, cx| window.has_active_dialog(cx)) {
        return;
    }
    for _ in 0..8 {
        draw(cx);
        let Some(row) = lookup(cx, selector) else {
            return;
        };
        let Some(mut viewport) = lookup(cx, "settings-viewport") else {
            return;
        };
        // The selector belongs to Kit's scroll content; use the visible window height.
        viewport.origin.y = px(48.);
        viewport.size.height = cx.update(|window, _| window.viewport_size().height) - px(72.);
        if row.center().y > viewport.top() + px(24.) && row.center().y < viewport.bottom() - px(24.)
        {
            return;
        }
        cx.simulate_event(ScrollWheelEvent {
            position: viewport.center(),
            delta: ScrollDelta::Pixels(point(px(0.), viewport.center().y - row.center().y)),
            ..Default::default()
        });
    }
    draw(cx);
}
