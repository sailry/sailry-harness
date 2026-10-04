use super::*;
use sailry_link::{
    Admission, CancellationToken, Link, NetworkScope, Pending, Subscription, Transport,
};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Fault, NodeId, Output, Request, RequestId, Topic};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
use std::time::{Duration, Instant};

pub(in crate::settings) struct Observed {
    pub computer: Mutex<Option<sailry_protocol::computer::Permissions>>,
    pub permission_requests: Mutex<Vec<sailry_protocol::computer::Permission>>,
    inner: Arc<dyn Transport>,
    pub mode: AtomicU8,
    pub read_mode: AtomicU8,
    pub read_cancelled: CancellationToken,
    pub catalog_hold: AtomicU8,
    pub catalog_entered: CancellationToken,
    pub catalog_cancelled: CancellationToken,
    pub requests: Mutex<Vec<RequestId>>,
    pub entered: CancellationToken,
    pub release: CancellationToken,
}

struct CatalogWait(CancellationToken);
impl Drop for CatalogWait {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(
                request.command,
                Command::ReadComputerPermissions | Command::RequestComputerPermission { .. }
            ) {
                let value = self.computer.lock().unwrap().clone();
                if let Some(value) = value {
                    if let Command::RequestComputerPermission { permission } = request.command {
                        self.permission_requests.lock().unwrap().push(permission);
                    }
                    let (sender, completion) = tokio::sync::oneshot::channel();
                    sender.send(Ok(Output::ComputerPermissions(value))).unwrap();
                    return Ok(Admission {
                        receipt: sailry_protocol::Receipt {
                            id: request.id,
                            durable: false,
                        },
                        completion,
                    });
                }
            }
            if matches!(
                request.command,
                Command::SearchPluginCatalog { .. } | Command::DiscoverSkills { .. }
            ) {
                match self.catalog_hold.swap(0, Ordering::SeqCst) {
                    1 => {
                        let admission = self.inner.dispatch(request).await?;
                        let result = admission.completion.await.unwrap();
                        let _waiting = CatalogWait(self.catalog_cancelled.clone());
                        self.catalog_entered.cancel();
                        self.release.cancelled().await;
                        let (sender, completion) = tokio::sync::oneshot::channel();
                        sender.send(result).unwrap();
                        return Ok(Admission {
                            receipt: admission.receipt,
                            completion,
                        });
                    }
                    2 => {
                        return Err(Fault::new(
                            sailry_protocol::ErrorCode::Unavailable,
                            "injected catalog failure",
                        ));
                    }
                    _ => {}
                }
            }
            if matches!(
                request.command,
                Command::ReadPlugin { .. } | Command::ReadCatalogPluginInfo { .. }
            ) {
                match self.read_mode.swap(0, Ordering::SeqCst) {
                    1 => {
                        return Err(Fault::new(
                            sailry_protocol::ErrorCode::Unavailable,
                            "injected plugin read failure",
                        ));
                    }
                    2 => {
                        let _waiting = CatalogWait(self.read_cancelled.clone());
                        self.entered.cancel();
                        self.release.cancelled().await;
                    }
                    _ => {}
                }
                return self.inner.dispatch(request).await;
            }
            if !matches!(
                request.command,
                Command::InstallPlugin { .. }
                    | Command::InstallPluginSource { .. }
                    | Command::InstallBundledPlugin { .. }
                    | Command::InstallPluginUpload { .. }
                    | Command::InstallSkill { .. }
                    | Command::InstallMcp { .. }
                    | Command::SetPluginEnabled { .. }
                    | Command::RemovePlugin { .. }
                    | Command::SavePluginSettings { .. }
                    | Command::SavePluginMcp { .. }
                    | Command::BeginMcpLogin { .. }
                    | Command::CompleteMcpLogin { .. }
                    | Command::CancelMcpLogin { .. }
                    | Command::RevokeMcpAuthorization { .. }
            ) {
                return self.inner.dispatch(request).await;
            }
            self.requests.lock().unwrap().push(request.id);
            let mode = self.mode.swap(0, Ordering::SeqCst);
            let admission = self.inner.dispatch(request).await?;
            if mode == 0 {
                return Ok(admission);
            }
            let result = admission.completion.await.unwrap();
            if mode == 1 {
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::Unavailable,
                    "injected plugin response loss",
                ));
            }
            self.entered.cancel();
            self.release.cancelled().await;
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender.send(result).unwrap();
            Ok(Admission {
                receipt: admission.receipt,
                completion,
            })
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn open(
        &self,
        stream: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<Box<dyn sailry_link::ByteStream>, Fault>> {
        self.inner.open(stream)
    }
}

pub(in crate::settings) struct Fixture {
    pub directory: tempfile::TempDir,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub node: Node,
    pub other: Node,
    controller: Link,
    pub transport: Arc<Observed>,
    pub client: sailry_client::Client,
    pub worktree: sailry_protocol::WorktreeId,
    pub initial_packages: Vec<sailry_protocol::plugin::Summary>,
}

impl Fixture {
    pub fn new(remote: bool) -> Self {
        Self::with_source(remote, None)
    }

    pub(in crate::settings) fn with_skill_source(remote: bool, endpoint: &str) -> Self {
        Self::with_source(remote, Some(endpoint))
    }

    fn with_source(remote: bool, endpoint: Option<&str>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(async {
                match endpoint {
                    Some(endpoint) => {
                        Node::start_with_skill_source(directory.path().join("node"), endpoint).await
                    }
                    None => Node::start(directory.path().join("node")).await,
                }
            })
            .unwrap();
        let other = runtime
            .block_on(Node::start(directory.path().join("other")))
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
        let client = sailry_client::Client::new(inner.clone());
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Plugin source".into(),
                path: root.to_str().unwrap().into(),
            })))
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.project == Some(project.id))
            .unwrap()
            .id;
        let transport = Arc::new(Observed {
            computer: Mutex::new(None),
            permission_requests: Mutex::new(Vec::new()),
            inner,
            mode: AtomicU8::new(0),
            read_mode: AtomicU8::new(0),
            read_cancelled: CancellationToken::new(),
            catalog_hold: AtomicU8::new(0),
            catalog_entered: CancellationToken::new(),
            catalog_cancelled: CancellationToken::new(),
            requests: Mutex::new(Vec::new()),
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
        });
        Self {
            directory,
            runtime,
            node,
            other,
            controller,
            transport,
            client,
            worktree,
            initial_packages: snapshot.plugins,
        }
    }

    pub fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| crate::shell::Shell::new(window, cx));
            shell.update(cx, |shell, cx| {
                shell.navigate(crate::preview::Page::Settings, window, cx)
            });
            let workspace = shell.read(cx).settings.clone();
            workspace.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    self.transport.clone(),
                    self.runtime.clone(),
                    "Fixture Node".into(),
                    cx,
                );
                workspace.select(crate::settings::Section::Plugins, cx);
            });
            owner = Some(workspace.clone());
            Root::new(shell, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| {
            owner.read(cx).provider_link.as_ref().unwrap().connected
        });
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        (owner, visual)
    }

    pub fn execute(&self, command: Command) -> Output {
        self.runtime
            .block_on(self.client.execute(self.client.prepare(command)))
            .unwrap()
    }

    pub fn plugins(&self) -> Vec<sailry_protocol::plugin::Summary> {
        self.public_packages(false)
            .into_iter()
            .filter(|entry| {
                !self
                    .initial_packages
                    .iter()
                    .any(|initial| initial.name == entry.name)
            })
            .collect()
    }

    pub fn public_packages(&self, other: bool) -> Vec<sailry_protocol::plugin::Summary> {
        let other = other.then(|| sailry_client::Client::new(self.other.local()));
        let client = other.as_ref().unwrap_or(&self.client);
        let Output::Plugins(plugins) = self
            .runtime
            .block_on(client.execute(client.prepare(Command::ListPlugins)))
            .unwrap()
        else {
            panic!("plugins expected")
        };
        plugins
    }

    pub fn package(&self, version: &str) {
        let root = self
            .directory
            .path()
            .join("project/package/skills/analysis");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("SKILL.md"),
            "---\nname: analysis\ndescription: Inspect changes\n---\nComplete skill\n",
        )
        .unwrap();
        let package = root.parent().unwrap().parent().unwrap();
        std::fs::create_dir_all(package.join(sailry_protocol::plugin::NAMESPACE)).unwrap();
        std::fs::write(package.join("plugin.json"), serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":"example", "version":version}).to_string()).unwrap();
        std::fs::write(package.join("mcp.json"), serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", "mcpServers":{"native":{"type":"stdio","command":"sailry_isolated_missing_executable"}}}).to_string()).unwrap();
    }

    pub fn archive(&self) -> std::path::PathBuf {
        use std::io::Write;
        let path = self.directory.path().join("selected.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for name in ["plugin.json", "mcp.json", "skills/analysis/SKILL.md"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(
                &std::fs::read(self.directory.path().join("project/package").join(name)).unwrap(),
            )
            .unwrap();
        }
        zip.finish().unwrap();
        path
    }

    pub fn bind(&self, owner: &Entity<Workspace>, cx: &mut VisualTestContext, other: bool) {
        let transport: Arc<dyn Transport> = if other {
            self.other.local()
        } else {
            self.transport.clone()
        };
        cx.update(|_, cx| {
            owner.update(cx, |owner, cx| {
                owner.bind_providers(transport, self.runtime.clone(), "Selected Node".into(), cx)
            })
        });
        wait(cx, |cx| {
            owner.read(cx).provider_link.as_ref().unwrap().connected
        });
    }

    pub fn close(self, cx: &mut VisualTestContext) {
        cx.update(|window, _| window.remove_window());
        self.runtime.block_on(self.node.shutdown()).unwrap();
        self.runtime.block_on(self.other.shutdown()).unwrap();
        self.runtime.block_on(self.controller.close()).unwrap();
    }
}

pub(in crate::settings) fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    rust_i18n::set_locale("en");
}

#[track_caller]
pub(in crate::settings) fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "plugin UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(in crate::settings) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
}

#[track_caller]
pub(in crate::settings) fn shown(
    cx: &mut VisualTestContext,
    selector: &'static str,
    expected: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        if cx.debug_bounds(selector).is_some() == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "plugin selector deadline: {selector}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[track_caller]
pub(in crate::settings) fn shown_settings(
    owner: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    selector: &'static str,
    expected: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        let state = cx.update(|_, cx| {
            owner
                .read(cx)
                .plugin_settings_panel()
                .map(|panel| crate::plugins::diagnostics(&panel, cx))
        });
        if state
            .as_ref()
            .is_some_and(|state| state.contains(selector) == expected)
            && cx.debug_bounds("plugin-panel").is_some()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "plugin settings selector deadline: {selector}; {state:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(in crate::settings) fn tap(cx: &mut VisualTestContext, selector: &'static str) {
    if cx.update(|window, cx| window.has_active_dialog(cx)) {
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    }
    if selector == "plugin-retry" {
        cx.executor().advance_clock(Duration::from_millis(350));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(350));
    }
    draw(cx);
    // The native catalog can extend below the viewport. Exercise its real scroll
    // container before clicking package actions instead of clicking clipped rows.
    for _ in 0..4 {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing {selector}"));
        let viewport = cx.update(|window, _| window.viewport_size());
        if bounds.center().y >= px(64.) && bounds.center().y < viewport.height - px(16.) {
            break;
        }
        cx.simulate_event(ScrollWheelEvent {
            position: point(bounds.center().x, viewport.height / 2.),
            delta: ScrollDelta::Pixels(point(px(0.), viewport.height / 2. - bounds.center().y)),
            touch_phase: TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        draw(cx);
    }
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

pub(in crate::settings) fn menu(cx: &mut VisualTestContext, selector: &'static str, index: usize) {
    tap(cx, selector);
    for _ in 0..=index {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    draw(cx);
}

pub(in crate::settings) fn plugin_menu(
    cx: &mut VisualTestContext,
    selector: &'static str,
    index: usize,
) {
    tap(cx, selector);
    cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    draw(cx);
    // Start from Uninstall so a disabled Settings item does not shift the selection.
    for _ in index..3 {
        cx.simulate_keystrokes("up");
    }
    cx.simulate_keystrokes("enter");
    draw(cx);
}

pub(in crate::settings) fn input(cx: &mut VisualTestContext, selector: &'static str, text: &str) {
    tap(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(text);
    draw(cx);
}

pub(in crate::settings) fn fill(cx: &mut VisualTestContext, fixture: &Fixture) {
    select(cx, fixture.archive());
    wait(cx, |_| !fixture.plugins().is_empty());
}

pub(in crate::settings) fn select(cx: &mut VisualTestContext, path: std::path::PathBuf) {
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && options.directories && !options.multiple);
        Some(vec![path])
    });
}
