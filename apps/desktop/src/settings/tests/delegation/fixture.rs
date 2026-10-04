use super::*;
use sailry_link::{
    Admission, CancellationToken, Link, NetworkScope, Pending, Subscription, Transport,
};
use sailry_node_runtime::Node;
use sailry_protocol::{
    Fault, NodeId, Request, RequestId, Topic,
    conversation::{Model, ModelApi, Provider},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::{Duration, Instant};

pub(super) struct Observed {
    inner: Arc<dyn Transport>,
    pub mode: AtomicU8,
    inspect_failed: AtomicBool,
    pub requests: Mutex<Vec<RequestId>>,
    pub entered: CancellationToken,
    pub release: CancellationToken,
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::InspectRequest { .. })
                && self.inspect_failed.swap(false, Ordering::SeqCst)
            {
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::Unavailable,
                    "injected role inspection failure",
                ));
            }
            if !matches!(
                request.command,
                Command::PutRole { .. } | Command::RemoveRole { .. }
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
                self.inspect_failed.store(true, Ordering::SeqCst);
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::Unavailable,
                    "injected role response loss",
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
}

pub(super) struct Fixture {
    _directory: tempfile::TempDir,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub node: Node,
    pub other: Node,
    controller: Link,
    pub transport: Arc<Observed>,
    pub client: sailry_client::Client,
    pub provider: Provider,
}

impl Fixture {
    pub fn new(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(Node::start(directory.path().join("node")))
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
        let Output::Provider(provider) = runtime
            .block_on(client.execute(client.prepare(Command::PutProvider {
                provider: Provider {
                    options: None,
                    id: sailry_protocol::ProviderId::new(),
                    revision: 0,
                    name: "Fixture provider".into(),
                    api: ModelApi::ChatCompletions,
                    authentication: sailry_protocol::Authentication::ApiKey,
                    endpoint: "http://127.0.0.1:9/v1".into(),
                    enabled: true,
                    credential: None,
                    default_model: "fixture-model".into(),
                    models: vec![Model {
                        id: "fixture-model".into(),
                        context: 4096,
                        output: 128,
                        vision: false,
                        tools: true,
                        reasoning: true,
                        web_search: false,
                        generates: vec![],
                        efforts: vec![sailry_protocol::Effort::Low, sailry_protocol::Effort::High],
                        custom_efforts: false,
                        default_effort: sailry_protocol::Effort::High,
                    }],
                },
                expected_revision: 0,
            })))
            .unwrap()
        else {
            panic!("provider expected")
        };
        let transport = Arc::new(Observed {
            inner,
            mode: AtomicU8::new(0),
            inspect_failed: AtomicBool::new(false),
            requests: Mutex::new(Vec::new()),
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
        });
        Self {
            _directory: directory,
            runtime,
            node,
            other,
            controller,
            transport,
            client,
            provider,
        }
    }

    pub fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        self.install();
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
                workspace.open_plugin_settings("delegation", cx);
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
        shown(&owner, visual, "role-settings", true);
        wait(visual, |cx| {
            owner.read(cx).plugin_settings_panel().is_some_and(|panel| {
                crate::plugins::diagnostics(&panel, cx).contains(":disabled[Bool(false)]")
            })
        });
        (owner, visual)
    }

    fn install(&self) {
        let root = self._directory.path().join("package-source");
        std::fs::create_dir(&root).unwrap();
        let package = root.join("delegation");
        crate::plugins::fixture::copy_package(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delegation"),
            &package,
        );
        let path = package.join("dev.sailry.platform/desktop/view.js");
        let source = std::fs::read_to_string(&path).unwrap().replace(
            "return div().id(id).child(control);",
            "return Anchor.new(id).child(control);",
        );
        let source = [
            "role-field-role_id",
            "role-field-settings_name",
            "role-max-turns",
            "role-instructions",
        ]
        .iter()
        .fold(source, |source, id| {
            source.replace(&format!("div().id('{id}')"), &format!("Anchor.new('{id}')"))
        });
        std::fs::write(
            path,
            format!("import {{Anchor}} from 'sailry/test';\n{source}"),
        )
        .unwrap();
        let Output::Project(project) = self.execute(Command::RegisterProject {
            name: "Role settings package".into(),
            path: root.to_str().unwrap().into(),
        }) else {
            panic!("project expected");
        };
        let Output::Snapshot(snapshot) = self.execute(Command::Snapshot) else {
            panic!("snapshot expected");
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.project == Some(project.id))
            .unwrap()
            .id;
        let Output::Plugin(current) = self.execute(Command::ReadPlugin {
            name: "delegation".into(),
        }) else {
            panic!("package expected");
        };
        let Output::Plugin(installed) = self.execute(Command::InstallPlugin {
            worktree,
            path: "delegation".into(),
            name: "delegation".into(),
            expected_revision: current.summary.revision,
        }) else {
            panic!("package expected");
        };
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
    }

    pub fn execute(&self, command: Command) -> Output {
        self.runtime
            .block_on(self.client.execute(self.client.prepare(command)))
            .unwrap()
    }

    pub fn roles(&self) -> Vec<sailry_protocol::role::Profile> {
        let Output::Roles(roles) = self.execute(Command::ListRoles) else {
            panic!("roles expected")
        };
        roles
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

pub(super) fn init(cx: &mut TestAppContext) {
    rust_i18n::set_locale("en");
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    rust_i18n::set_locale("en");
}

pub(super) fn preset(kind: &str) -> &'static str {
    match kind {
        "review" => {
            "Review code and changes within the assigned scope, focusing on correctness, edge cases, regression risks and existing conventions.\nRead relevant implementations and tests first, then report evidence-backed findings with locations, impact and recommendations. Do not modify files unless asked; state plainly when no issues are found."
        }
        "research" => {
            "Investigate the assigned objective, prioritizing existing materials, code and reliable primary sources.\nDistinguish confirmed facts, inferences and open questions, and provide concise conclusions with supporting evidence. Identify missing information when needed; do not invent results."
        }
        _ => unreachable!(),
    }
}

pub(super) fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "role UI deadline");
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

pub(super) fn shown(
    owner: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    selector: &str,
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
                .unwrap_or_default()
        });
        if state.contains(selector) == expected && !state.is_empty() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "role selector deadline: {selector}; {state}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn tap(cx: &mut VisualTestContext, selector: &str) {
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
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

pub(super) fn input(cx: &mut VisualTestContext, selector: &str, text: &str) {
    tap(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(text);
    draw(cx);
}

pub(super) fn copied(cx: &mut VisualTestContext) -> String {
    cx.simulate_keystrokes("secondary-a secondary-c");
    cx.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap())
}

pub(super) fn fill(cx: &mut VisualTestContext, key: &str) {
    input(cx, "role-field-role_id", key);
    input(cx, "role-field-settings_name", "Review 中文 🙂");
}

pub(super) fn choose(cx: &mut VisualTestContext, selector: &str, index: usize) {
    tap(cx, selector);
    cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    for _ in 0..=index {
        cx.simulate_keystrokes("down");
        draw(cx);
    }
    cx.simulate_keystrokes("enter");
    draw(cx);
}
