use super::*;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use tokio::{net::UnixStream, task::JoinHandle};

pub(super) use crate::models;

pub(super) struct Fixture {
    pub directory: tempfile::TempDir,
    pub node: Node,
    pub client: Arc<Client>,
    pub session: Session,
    pub model: models::Server,
    pub loss: Option<Arc<super::transport::Lost>>,
    controller: Option<Link>,
    stop: CancellationToken,
    service: JoinHandle<()>,
}

impl Fixture {
    pub async fn new(remote: bool, model: models::Server) -> Self {
        Self::setup(remote, model, false).await
    }

    pub async fn with_loss(remote: bool, model: models::Server) -> Self {
        Self::setup(remote, model, true).await
    }

    async fn setup(remote: bool, model: models::Server, lose: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport): (Option<Link>, Arc<dyn Transport>) = if remote {
            let controller =
                Link::controller(directory.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            let address = controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let transport = controller.handle().remote(address);
            (Some(controller), transport)
        } else {
            (None, node.local())
        };
        let loss = lose.then(|| Arc::new(super::transport::Lost::new(transport.clone())));
        let transport: Arc<dyn Transport> = match &loss {
            Some(loss) => loss.clone(),
            None => transport,
        };
        let client = Arc::new(Client::new(transport.clone()));
        for name in ["files", "delegation"] {
            let Output::Plugin(info) = execute(
                &client,
                Command::InstallBundledPlugin {
                    name: name.into(),
                    expected_revision: 0,
                },
            )
            .await
            else {
                panic!("fixture package expected")
            };
            assert!(info.issues.is_empty(), "{:?}", info.issues);
        }
        let provider = provider(&client, &model.endpoint).await;
        let Output::Project(project) = execute(
            &client,
            Command::RegisterProject {
                name: "Management fixture".into(),
                path: root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let Output::Session(session) = execute(
            &client,
            Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: None,
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let stop = CancellationToken::new();
        let listener = crate::control::listen(node.profile()).unwrap();
        let service = tokio::spawn(crate::control::serve(
            listener,
            node.link(),
            transport,
            stop.clone(),
        ));
        Self {
            directory,
            node,
            client,
            session,
            model,
            loss,
            controller,
            stop,
            service,
        }
    }

    pub fn root(&self) -> std::path::PathBuf {
        self.directory.path().join("project")
    }

    pub fn submit(&self, request: RequestId, wait: bool) -> Task {
        Task::Submit {
            version: 1,
            request,
            target: Target::Session {
                session: self.session.id,
                expected_revision: Some(self.session.revision),
            },
            message: "Complete the submitted task".into(),
            title: String::new(),
            wait,
            timeout: 10,
        }
    }

    pub async fn execute(&self, command: Command) -> Output {
        execute(&self.client, command).await
    }

    pub async fn page(&self) -> conversation::Page {
        self.client
            .read_conversation(self.session.id, None, 100)
            .await
            .unwrap()
            .page
    }

    pub async fn connect(&self, task: &Task) -> BufReader<UnixStream> {
        let mut stream = UnixStream::connect(self.node.profile().join("control.sock"))
            .await
            .unwrap();
        stream
            .write_all(&serde_json::to_vec(task).unwrap())
            .await
            .unwrap();
        stream.write_all(b"\n").await.unwrap();
        BufReader::new(stream)
    }

    pub async fn exchange(&self, task: &Task) -> Vec<Result> {
        let mut stream = self.connect(task).await;
        let mut responses = Vec::new();
        loop {
            let mut line = String::new();
            let count = tokio::time::timeout(Duration::from_secs(15), stream.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            if count == 0 {
                break;
            }
            responses.push(serde_json::from_str(&line).unwrap());
        }
        responses
    }

    pub async fn wait(&self, turn: TurnId, timeout: u64) -> Result {
        self.exchange(&Task::Wait {
            version: 1,
            session: self.session.id,
            turn,
            timeout,
        })
        .await
        .pop()
        .unwrap()
    }

    pub async fn close(self) {
        self.stop.cancel();
        self.service.await.unwrap();
        self.node.shutdown().await.unwrap();
        if let Some(controller) = self.controller {
            controller.close().await.unwrap();
        }
    }
}

pub(super) async fn receive(stream: &mut BufReader<UnixStream>) -> Result {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}

pub(super) async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

pub(super) async fn provider(client: &Client, endpoint: &str) -> conversation::Provider {
    let Output::Provider(provider) = execute(
        client,
        Command::PutProvider {
            expected_revision: 0,
            provider: conversation::Provider {
                preset: None,
                binding: None,
                consent: None,
                id: ProviderId::new(),
                revision: 0,
                name: "Scripted model".into(),
                api: conversation::ModelApi::ChatCompletions,
                authentication: Authentication::ApiKey,
                endpoint: endpoint.into(),
                enabled: true,
                models: vec![conversation::Model {
                    id: "fixture".into(),
                    display_name: None,
                    context: 4096,
                    output: 128,
                    capabilities: None,
                    vision: false,
                    tools: true,
                    reasoning: false,
                    web_search: false,
                    generates: Vec::new(),
                    efforts: Vec::new(),
                    custom_efforts: false,
                    default_effort: Effort::Default,
                }],
                default_model: "fixture".into(),
                credential: None,
                oauth: None,
                options: None,
            },
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    provider
}
