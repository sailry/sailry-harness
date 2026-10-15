use super::{provider, support};
use crate::conversation::live::*;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;

pub(crate) struct Fixture {
    pub(crate) directory: tempfile::TempDir,
    pub(crate) runtime: Arc<tokio::runtime::Runtime>,
    pub(crate) node: Node,
    pub(crate) controller: Option<Link>,
    pub(crate) binding: Binding,
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
    pub(crate) fn with_tools(remote: bool, tools: Vec<(String, serde_json::Value)>) -> Self {
        Self::with_server(remote, |runtime| {
            runtime.block_on(support::Server::tools(tools))
        })
    }

    pub(crate) fn with_server(
        remote: bool,
        server: impl FnOnce(&tokio::runtime::Runtime) -> support::Server,
    ) -> Self {
        Self::with_environment(remote, |_| None, server)
    }

    pub(crate) fn with_environment(
        remote: bool,
        setup: impl FnOnce(&std::path::Path) -> Option<sailry_node_runtime::EnvironmentFixture>,
        server: impl FnOnce(&tokio::runtime::Runtime) -> support::Server,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = match setup(directory.path()) {
            Some(environment) => runtime.block_on(Node::start_with_environment(
                directory.path().join("node"),
                environment,
            )),
            None => runtime.block_on(Node::start_isolated(directory.path().join("node"))),
        }
        .unwrap();
        let (controller, transport): (Option<Link>, Arc<dyn Transport>) = if remote {
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
            let transport = controller.handle().remote(address);
            (Some(controller), transport)
        } else {
            (None, node.local())
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
            binding,
        }
    }

    pub(crate) fn execute(&self, command: Command) -> Output {
        let client = &self.binding.client;
        self.runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    }

    pub(crate) fn close(self) {
        self.runtime.block_on(self.node.shutdown()).unwrap();
        if let Some(controller) = self.controller {
            self.runtime.block_on(controller.close()).unwrap();
        }
    }
}
