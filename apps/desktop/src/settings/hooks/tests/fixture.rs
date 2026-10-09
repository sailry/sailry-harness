use super::*;
use sailry_link::{Admission, Link, NetworkScope, Pending, Subscription, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Fault, NodeId, Output, Request, Topic, WorktreeId};
use std::{
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
            let tracked = matches!(request.command, Command::SaveHooks { .. });
            if tracked {
                self.requests.lock().unwrap().push(request.clone());
            }
            let dropped = tracked && self.drop_next.swap(false, Ordering::SeqCst);
            let mut admission = self.inner.dispatch(request).await?;
            if dropped {
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let completion = std::mem::replace(&mut admission.completion, receiver);
                tokio::spawn(async move {
                    let _ = completion.await;
                    drop(sender);
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
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let root = directory.path().canonicalize().unwrap().join("project");
        std::fs::create_dir(&root).unwrap();
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
        let transport = Arc::new(Observed {
            inner,
            requests: Mutex::new(Vec::new()),
            drop_next: AtomicBool::new(false),
        });
        let client = sailry_client::Client::new(transport.clone());
        for (name, path) in [
            ("Hooks", root.clone()),
            (
                "Other project",
                directory.path().canonicalize().unwrap().join("second"),
            ),
        ] {
            std::fs::create_dir_all(&path).unwrap();
            runtime
                .block_on(client.execute(client.prepare(Command::RegisterProject {
                    name: name.into(),
                    path: path.to_str().unwrap().into(),
                })))
                .unwrap();
        }
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
            .find(|tree| tree.id != worktree)
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

    pub fn settings(&self) -> Settings {
        let Output::HookSettings(state) = self.execute(Command::ReadHooks {
            worktree: self.worktree,
        }) else {
            panic!("hook settings expected")
        };
        assert_eq!(state.worktree, self.worktree);
        state.settings
    }

    pub fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let settings = cx.new(|cx| Workspace::new(window, cx));
            settings.update(cx, |settings, cx| {
                settings.section = super::super::super::Section::Hooks;
                settings.bind_hooks(
                    self.transport.clone(),
                    self.runtime.clone(),
                    "Fixture Node".into(),
                    Some(self.worktree),
                    window,
                    cx,
                );
            });
            owner = Some(settings.clone());
            Root::new(settings, window, cx)
        });
        let owner = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1100.), px(1200.)));
        wait(visual, |cx| {
            selected(&owner, cx).read(cx).saved.is_some() && !selected(&owner, cx).read(cx).loading
        });
        (owner, visual)
    }

    pub fn bind(
        &self,
        owner: &Entity<Workspace>,
        cx: &mut VisualTestContext,
        other: bool,
        tree: Option<WorktreeId>,
    ) {
        let transport: Arc<dyn Transport> = if other {
            self.other.local()
        } else {
            self.transport.clone()
        };
        cx.update(|window, cx| {
            owner.update(cx, |settings, cx| {
                settings.bind_hooks(
                    transport,
                    self.runtime.clone(),
                    "Selected Node".into(),
                    tree,
                    window,
                    cx,
                )
            })
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
    cx.executor().allow_parking();
    rust_i18n::set_locale("en");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
}
pub(super) fn selected(owner: &Entity<Workspace>, cx: &App) -> Entity<Panel> {
    let state = &owner.read(cx).hooks;
    state.panels[&state.selected.unwrap()].clone()
}
pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
}
pub(super) fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        draw(cx);
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "hook settings deadline");
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
pub(super) fn input(cx: &mut VisualTestContext, selector: &str, value: &str) {
    tap(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(value);
    draw(cx);
}
