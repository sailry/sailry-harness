use super::{provider, support, wait};
use crate::conversation::live::*;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use std::time::Duration;

pub(crate) struct Fixture {
    pub(crate) directory: tempfile::TempDir,
    pub(crate) runtime: Arc<tokio::runtime::Runtime>,
    pub(crate) node: Node,
    pub(crate) controller: Link,
    pub(crate) transport: Arc<dyn Transport>,
    pub(crate) binding: Binding,
    pub(crate) session: Session,
    pub(crate) server: support::Server,
}

impl Fixture {
    pub(crate) fn files_context(&self) -> sailry_protocol::plugin::Context {
        let Output::Plugin(mut info) = self.execute(Command::ReadPlugin {
            name: "files".into(),
        }) else {
            panic!("Files package expected");
        };
        if !info.summary.enabled {
            let Output::Plugin(enabled) = self.execute(Command::SetPluginEnabled {
                name: info.summary.name,
                expected_revision: info.summary.revision,
                enabled: true,
            }) else {
                panic!("enabled package expected");
            };
            info = enabled;
        }
        sailry_protocol::plugin::Context {
            package: info.summary.reference(),
            worktree: self.binding.worktree,
            session: None,
            turn: None,
            invocation: None,
            surface: Default::default(),
        }
    }
    pub(crate) fn task_requests(&self) -> usize {
        self.server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| !support::compaction::is_summary(request))
            .count()
    }

    pub(crate) fn with_tools(remote: bool, tools: Vec<(String, serde_json::Value)>) -> Self {
        Self::with_server(remote, |runtime| {
            runtime.block_on(support::Server::tools(tools))
        })
    }

    pub(crate) fn with_server(
        remote: bool,
        server: impl FnOnce(&tokio::runtime::Runtime) -> support::Server,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(Node::start(directory.path().join("node")))
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
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Arc::new(Client::new(transport.clone()));
        // Keep model fixtures independent of shipped external services.
        for name in ["context7", "github"] {
            runtime
                .block_on(client.execute(client.prepare(Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: 1,
                    enabled: false,
                })))
                .unwrap();
        }
        let server = server(&runtime);
        let mut model = provider(&server.endpoint, "approval-model");
        model.models[0].tools = true;
        runtime
            .block_on(client.execute(client.prepare(Command::SaveProvider {
                provider: model.clone(),
                expected_revision: 0,
                secret: None,
            })))
            .unwrap();
        let Output::Project(project) = runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Approval fixture".into(),
                path: root.to_str().unwrap().into(),
            })))
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(session) = runtime
            .block_on(client.execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: model.id,
                    model: model.default_model,
                    effort: Effort::High,
                    mode: sailry_protocol::WorkMode::Code,
                    permission: sailry_protocol::Permission::Ask,
                    credential: None,
                }),
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        let binding = Binding {
            client: client.clone(),
            defaults: client,
            runtime: runtime.clone(),
            project: Some(project.id),
            worktree: Some(session.worktree),
            host: "Fixture Node".into(),
            project_name: "Approval fixture".into(),
            branch: "main".into(),
        };
        Self {
            directory,
            runtime,
            node,
            controller,
            transport,
            binding,
            session,
            server,
        }
    }

    pub(crate) fn execute(&self, command: Command) -> Output {
        let client = &self.binding.client;
        self.runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    }

    pub(crate) fn start(&self) {
        self.execute(Command::SubmitTurn {
            session: self.session.id,
            expected_revision: 1,
            message: "Run the fixture tools".into(),
        });
    }

    pub(crate) fn close(self) {
        self.runtime.block_on(self.node.shutdown()).unwrap();
        self.runtime.block_on(self.controller.close()).unwrap();
    }
}

pub(crate) struct Harness(pub(crate) Entity<View>);
impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

pub(crate) fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    rust_i18n::set_locale("en");
}

pub(crate) fn open(
    cx: &mut TestAppContext,
    binding: Binding,
    session: Session,
) -> (Entity<View>, &mut VisualTestContext) {
    open_session(cx, binding, Some(session))
}

pub(crate) fn open_session(
    cx: &mut TestAppContext,
    binding: Binding,
    session: Option<Session>,
) -> (Entity<View>, &mut VisualTestContext) {
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| View::new(binding, session, window, cx));
        entity = Some(view.clone());
        let harness = cx.new(|_| Harness(view));
        Root::new(harness, window, cx)
    });
    (entity.unwrap(), visual)
}

pub(crate) fn hover(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx.debug_bounds(selector).unwrap();
    move_pointer(cx, bounds.center());
}

pub(crate) fn leave(cx: &mut VisualTestContext) {
    move_pointer(cx, point(px(1.), px(1.)));
}

fn move_pointer(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_mouse_move(position, None, Modifiers::default());
    for _ in 0..12 {
        cx.executor().advance_clock(Duration::from_millis(100));
        wait(cx, |_| true);
    }
}

pub(crate) fn tap(cx: &mut VisualTestContext, selector: &str) {
    if matches!(
        selector,
        "chat-retry"
            | "asset-retry"
            | "chat-sync-retry"
            | "live-approval-retry"
            | "queue-retry"
            | "live-search-retry"
            | "live-load-older"
    ) {
        crate::feedback::tests::settle(cx);
    }
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let selector = Box::leak(selector.to_owned().into_boxed_str());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .expect("control after pointer layout");
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

pub(crate) use crate::conversation::live::attachments::image_testing::{
    ImageView, finish_download,
};
