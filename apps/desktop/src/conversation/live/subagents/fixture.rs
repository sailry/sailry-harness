use crate::backend::Services;
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{
    Command, Output, ProviderId, RoleId, Session,
    conversation::{Model, ModelApi, Provider},
    role,
};
use std::sync::Arc;

use crate::agent_fixture as support;

pub(crate) struct Fixture {
    pub directory: tempfile::TempDir,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub local: Node,
    pub remote: Node,
    pub client: Arc<Client>,
    pub session: Session,
    pub parent: support::Server,
    pub child: support::Server,
}

impl Fixture {
    pub fn new(remote: bool) -> Self {
        Self::with_children(remote, 2)
    }

    pub fn with_children(remote: bool, count: usize) -> Self {
        Self::build(remote, count, false)
    }

    pub fn with_worktrees(remote: bool) -> Self {
        Self::build(remote, 2, true)
    }

    fn build(remote: bool, count: usize, isolated: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let local = runtime
            .block_on(Node::start(directory.path().join("local")))
            .unwrap();
        let node = runtime
            .block_on(Node::start(directory.path().join("remote")))
            .unwrap();
        let address = runtime
            .block_on(local.link().pair(node.link().invite().unwrap().ticket()))
            .unwrap();
        let client = Arc::new(Client::new(if remote {
            local.link().remote(address)
        } else {
            local.local()
        }));
        let execute = |command| {
            runtime
                .block_on(client.execute(client.prepare(command)))
                .unwrap()
        };
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = execute(Command::RegisterProject {
            name: "Child conversation fixture".into(),
            path: root.to_str().unwrap().into(),
        }) else {
            panic!("project expected")
        };
        let mut targets = Vec::new();
        if isolated {
            let repo = git2::Repository::init(&root).unwrap();
            let tree = repo
                .find_tree(repo.index().unwrap().write_tree().unwrap())
                .unwrap();
            let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
            let commit = repo
                .commit(Some("HEAD"), &author, &author, "Initial", &tree, &[])
                .unwrap();
            for index in 0..count {
                let Output::Worktree(tree) = execute(Command::CreateWorktree {
                    project: project.id,
                    path: directory
                        .path()
                        .join(format!("child-{index}"))
                        .to_str()
                        .unwrap()
                        .into(),
                    branch: format!("child-{index}"),
                    commit: commit.to_string(),
                }) else {
                    panic!("worktree expected")
                };
                targets.push(tree.id);
            }
        }
        let parent = runtime.block_on(support::Server::parallel(vec![
            (crate::agent_fixture::plugin_tool("progress", "update_plan"), serde_json::json!({"title": "Review", "steps": [{"description": "Inspect files", "state": "in_progress"}]})),
            (crate::agent_support::plugin_tool("delegation", "spawn_agent"), serde_json::json!({"role": "review", "title": "Inspect files", "task": "Inspect Unicode paths 中文 🙂 and preserve the original content"})),
            (crate::agent_support::plugin_tool("delegation", "spawn_agent"), serde_json::json!({"role": "review", "title": "Check lifecycle", "task": "Check the independent child lifecycle and cancellation"})),
        ].into_iter().take(count + 1).enumerate().map(|(index, (name, mut args))| {
            if isolated && index > 0 { args["worktree"] = serde_json::json!(targets[index - 1]); }
            (name, args)
        }).collect()));
        let child = runtime.block_on(support::Server::tools(vec![(crate::agent_fixture::plugin_tool("files", "write_file"), serde_json::json!({
            "path": "child.txt", "text": "Delegated Unicode content 中文 🙂", "expected_revision": null
        }))]));
        let execute = |command| {
            runtime
                .block_on(client.execute(client.prepare(command)))
                .unwrap()
        };
        let provider = |endpoint: &str, model: &str| Provider {
            options: None,
            oauth: None,
            id: ProviderId::new(),
            revision: 0,
            name: model.into(),
            api: ModelApi::ChatCompletions,
            authentication: sailry_protocol::Authentication::ApiKey,
            endpoint: endpoint.into(),
            enabled: true,
            credential: None,
            default_model: model.into(),
            models: vec![Model {
                id: model.into(),
                context: 16000,
                output: 2048,
                tools: true,
                vision: false,
                reasoning: false,
                web_search: false,
                generates: vec![],
                efforts: vec![],
                custom_efforts: false,
                default_effort: sailry_protocol::Effort::Default,
            }],
        };
        let parent_model = provider(&parent.endpoint, "parent-fixture");
        let child_model = provider(&child.endpoint, "child-fixture");
        for model in [&parent_model, &child_model] {
            execute(Command::PutProvider {
                provider: model.clone(),
                expected_revision: 0,
            });
        }
        let Output::Role(role) = execute(Command::PutRole {
            role: role::Profile {
                appearance: None,
                id: RoleId::new(),
                revision: 0,
                key: "review".into(),
                name: "Review".into(),
                description: "Independent review".into(),
                instructions: "Work on the assigned task".into(),
                model: Some(role::Model {
                    provider: child_model.id,
                    model: child_model.default_model,
                    effort: None,
                }),
                max_turns: Some(5),
                skills: vec![],
            },
            expected_revision: 0,
        }) else {
            panic!("role expected")
        };
        let Output::Session(session) = execute(Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(sailry_protocol::SessionConfig {
                assistant: None,
                resource: None,
                provider: parent_model.id,
                model: parent_model.default_model,
                effort: sailry_protocol::Effort::Medium,
                mode: sailry_protocol::WorkMode::Code,
                permission: sailry_protocol::Permission::Ask,
                credential: None,
            }),
        }) else {
            panic!("session expected")
        };
        let Output::Session(session) = execute(Command::SetSessionRoles {
            session: session.id,
            expected_revision: session.revision,
            roles: vec![role.reference()],
        }) else {
            panic!("session expected")
        };
        Self {
            directory,
            runtime,
            local,
            remote: node,
            client,
            session,
            parent,
            child,
        }
    }

    pub fn services(&self) -> Services {
        Services {
            runtime: self.runtime.clone(),
            local: self.local.local(),
            link: self.local.link(),
            relay_enabled: false,
        }
    }

    pub fn execute(&self, command: Command) -> Output {
        self.runtime
            .block_on(self.client.execute(self.client.prepare(command)))
            .unwrap()
    }

    pub fn start(&self) {
        self.execute(Command::SubmitTurn {
            session: self.session.id,
            expected_revision: self.session.revision,
            message: "Run two independent reviews".into(),
        });
    }

    pub fn page(&self, session: sailry_protocol::SessionId) -> sailry_protocol::conversation::Page {
        let Output::Conversation(page) = self.execute(Command::ReadConversation {
            session,
            before: None,
            limit: 100,
        }) else {
            panic!("conversation expected")
        };
        page.page
    }

    pub fn close(self) {
        self.runtime.block_on(self.local.shutdown()).unwrap();
        self.runtime.block_on(self.remote.shutdown()).unwrap();
    }
}
