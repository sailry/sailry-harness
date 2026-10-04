use crate::agent_fixture::Server;
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{Model, ModelApi, Provider},
    *,
};
use std::sync::Arc;

pub(crate) struct Fixture {
    _directory: tempfile::TempDir,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub nodes: [Node; 2],
    pub sessions: [Session; 2],
    servers: [Server; 2],
}
impl Fixture {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let nodes = ["local", "remote"].map(|name| {
            runtime
                .block_on(Node::start(directory.path().join(name)))
                .unwrap()
        });
        runtime
            .block_on(
                nodes[0]
                    .link()
                    .pair(nodes[1].link().invite().unwrap().ticket()),
            )
            .unwrap();
        let servers = [0, 1].map(|_| runtime.block_on(Server::tools(vec![("ask_user".into(), serde_json::json!({"prompt":"Continue", "input":{"kind":"text","multiline":false,"max_bytes":128}}))])));
        let sessions = [0, 1].map(|index| {
            let root = directory.path().join(format!("project-{index}"));
            std::fs::create_dir(&root).unwrap();
            git2::Repository::init(&root).unwrap();
            let client = Client::new(nodes[index].local());
            let execute = |command| {
                runtime
                    .block_on(client.execute(client.prepare(command)))
                    .unwrap()
            };
            // Keep model fixtures independent of shipped external services.
            for name in ["context7", "github"] {
                execute(Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: 1,
                    enabled: false,
                });
            }
            let provider = Provider {
                options: None,
                id: ProviderId::new(),
                revision: 0,
                name: "Activity fixture".into(),
                api: ModelApi::ChatCompletions,
                authentication: Authentication::ApiKey,
                endpoint: servers[index].endpoint.clone(),
                enabled: true,
                credential: None,
                default_model: "fixture-a".into(),
                models: vec![Model {
                    id: "fixture-a".into(),
                    context: 16000,
                    output: 2048,
                    vision: false,
                    tools: true,
                    reasoning: false,
                    web_search: false,
                    generates: vec![],
                    efforts: vec![],
                    custom_efforts: false,
                    default_effort: Effort::Default,
                }],
            };
            execute(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: 0,
            });
            let Output::Project(project) = execute(Command::RegisterProject {
                name: format!("Activity project {index}"),
                path: root.to_str().unwrap().into(),
            }) else {
                panic!("project expected")
            };
            let Output::Session(session) = execute(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    model: "fixture-a".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    permission: Permission::Ask,
                    credential: None,
                }),
            }) else {
                panic!("session expected")
            };
            session
        });
        Self {
            _directory: directory,
            runtime,
            nodes,
            sessions,
            servers,
        }
    }
    pub fn services(&self) -> crate::backend::Services {
        crate::backend::Services {
            runtime: self.runtime.clone(),
            link: self.nodes[0].link(),
            local: self.nodes[0].local(),
            relay_enabled: false,
        }
    }
    pub fn submit(&self, index: usize) {
        let client = Client::new(self.nodes[index].local());
        self.runtime
            .block_on(client.execute(client.prepare(Command::SubmitTurn {
                session: self.sessions[index].id,
                expected_revision: 1,
                message: format!("Review workspace changes {index}").into(),
            })))
            .unwrap();
    }
    pub fn answer(&self, index: usize) {
        let client = Client::new(self.nodes[index].local());
        self.runtime.block_on(async {
            let history = client
                .read_conversation(self.sessions[index].id, None, 20)
                .await
                .unwrap();
            let question = history
                .page
                .questions
                .iter()
                .find(|q| q.state == conversation::question::State::Pending)
                .unwrap();
            client
                .execute(client.prepare(Command::ResolveQuestion {
                    session: self.sessions[index].id,
                    question: question.id,
                    response: conversation::question::Response::Answer(
                        conversation::question::Answer::Text("Continue".into()),
                    ),
                }))
                .await
                .unwrap();
        });
    }
    pub fn close(self) {
        for node in self.nodes {
            self.runtime.block_on(node.shutdown()).unwrap();
        }
        drop(self.servers);
    }
}
