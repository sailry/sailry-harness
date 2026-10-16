use super::*;
use futures::{SinkExt, StreamExt, channel::mpsc as lines};
use sailry_link::{Admission, Link, NetworkScope, Pending, Subscription, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{
    Authentication, Effort, Permission, ProviderId, SessionConfig, Update, WorkMode,
    conversation::{self, Change, Status},
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) use crate::models;

pub(super) struct Fixture {
    pub directory: tempfile::TempDir,
    root: PathBuf,
    pub node: Node,
    pub client: Arc<Client>,
    pub session: Session,
    pub model: models::Server,
    pub loss: Arc<std::sync::Mutex<Option<Request>>>,
    controller: Option<Link>,
    stop: CancellationToken,
    control: JoinHandle<()>,
}

impl Fixture {
    pub async fn new(remote: bool, model: models::Server, lose: bool) -> Self {
        Self::start(remote, model, lose.then_some(Loss::Admission)).await
    }

    pub async fn with_decision_loss(remote: bool, model: models::Server) -> Self {
        Self::start(remote, model, Some(Loss::Decision)).await
    }

    async fn start(remote: bool, model: models::Server, lose: Option<Loss>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project 中文");
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, direct): (Option<Link>, Arc<dyn Transport>) = if remote {
            let controller =
                Link::controller(directory.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            let address = controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let direct = controller.handle().remote(address);
            (Some(controller), direct)
        } else {
            (None, node.local())
        };
        let client = Arc::new(Client::new(direct.clone()));
        for name in ["files", "delegation", "commands"] {
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
        let config = SessionConfig {
            assistant: None,
            resource: None,
            provider: provider.id,
            model: provider.default_model.clone(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Ask,
            credential: None,
        };
        execute(
            &client,
            Command::SetDefaults {
                expected_revision: 0,
                config: config.clone(),
            },
        )
        .await;
        let Output::Project(project) = execute(
            &client,
            Command::RegisterProject {
                name: "ACP project".into(),
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
                config: Some(config),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let loss = Arc::new(std::sync::Mutex::new(None));
        let transport: Arc<dyn Transport> = if let Some(kind) = lose {
            Arc::new(Lost {
                inner: direct,
                pending: AtomicBool::new(true),
                kind,
                record: loss.clone(),
            })
        } else {
            direct
        };
        let stop = CancellationToken::new();
        let listener = crate::control::listen(node.profile()).unwrap();
        let control = tokio::spawn(crate::control::serve(
            listener,
            node.link(),
            transport,
            stop.clone(),
        ));
        Self {
            directory,
            root,
            node,
            client,
            session,
            model,
            loss,
            controller,
            stop,
            control,
        }
    }

    pub fn root(&self) -> PathBuf {
        self.root.clone()
    }

    pub async fn editor(&self) -> Editor {
        let socket = crate::control::transport::Socket::connect(self.node.profile())
            .await
            .unwrap();
        Editor::new(Adapter::new(Arc::new(Client::new(Arc::new(socket))))).await
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

    pub async fn settled(&self) -> conversation::Page {
        let mut updates = self
            .client
            .subscribe_conversation(self.session.id)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let terminal = match updates.next().await.unwrap() {
                    Update::ConversationSnapshot(snapshot) => {
                        !snapshot.page.runs.is_empty()
                            && snapshot.page.runs.iter().all(|run| {
                                !matches!(run.status, Status::Queued | Status::Running | Status::Stopping)
                            })
                    }
                    Update::ConversationFrame(frame) => matches!(
                        frame.change,
                        Change::Run(run) if !matches!(run.status, Status::Queued | Status::Running | Status::Stopping)
                    ),
                    Update::ResetRequired => {
                        updates = self.client.subscribe_conversation(self.session.id).await.unwrap();
                        false
                    }
                    _ => false,
                };
                if terminal {
                    let page = self.page().await;
                    if !page.runs.is_empty()
                        && page.runs.iter().all(|run| {
                            !matches!(run.status, Status::Queued | Status::Running | Status::Stopping)
                        })
                    {
                        return page;
                    }
                }
            }
        })
        .await
        .unwrap()
    }

    pub async fn close(self) {
        self.stop.cancel();
        self.control.await.unwrap();
        self.node.shutdown().await.unwrap();
        if let Some(controller) = self.controller {
            controller.close().await.unwrap();
        }
    }
}

pub(super) struct Editor {
    input: lines::UnboundedSender<String>,
    output: lines::UnboundedReceiver<String>,
    service: JoinHandle<acp::Result<()>>,
    pub updates: Vec<Value>,
    pub permissions: Vec<Value>,
    responses: BTreeMap<u64, Value>,
}

impl Editor {
    async fn new(adapter: Arc<Adapter>) -> Self {
        let (input, incoming) = lines::unbounded::<String>();
        let (outgoing, output) = lines::unbounded::<String>();
        let component = sdk::Lines::new(
            outgoing.sink_map_err(std::io::Error::other),
            incoming.map(Ok),
        );
        let service = tokio::spawn(connection::serve(adapter, component));
        Self {
            input,
            output,
            service,
            updates: Vec::new(),
            permissions: Vec::new(),
            responses: BTreeMap::new(),
        }
    }

    pub fn send(&self, value: Value) {
        self.input.unbounded_send(value.to_string()).unwrap();
    }

    pub async fn next(&mut self) -> Value {
        serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(15), self.output.next())
                .await
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    }

    pub async fn request(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
        choice: Option<&str>,
    ) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        self.reply(id, choice).await
    }

    pub async fn reply(&mut self, id: u64, choice: Option<&str>) -> Value {
        if let Some(response) = self.responses.remove(&id) {
            return response;
        }
        loop {
            let frame = self.next().await;
            if frame.get("id") == Some(&json!(id)) && frame.get("method").is_none() {
                return frame;
            }
            if frame.get("method").is_none()
                && let Some(other) = frame["id"].as_u64()
            {
                self.responses.insert(other, frame);
                continue;
            }
            match frame["method"].as_str() {
                Some("session/update") => self.updates.push(frame["params"].clone()),
                Some("$/cancel_request") => {}
                Some("session/request_permission") => {
                    self.permissions.push(frame["params"].clone());
                    if let Some(choice) = choice {
                        self.send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{"outcome":{"outcome":"selected","optionId":choice}}}));
                    }
                }
                _ => panic!("unexpected ACP frame: {frame}"),
            }
        }
    }

    pub async fn initialize(&mut self) -> Value {
        self.request(
            1,
            "initialize",
            json!({"protocolVersion":1,"clientCapabilities":{}}),
            None,
        )
        .await
    }

    pub async fn load(&mut self, fixture: &Fixture) -> Value {
        let result = self
            .request(
                2,
                "session/load",
                json!({"sessionId":fixture.session.id,"cwd":fixture.root(),"mcpServers":[]}),
                None,
            )
            .await;
        assert!(result.get("result").is_some(), "{result}");
        result
    }

    pub async fn close(self) {
        drop(self.input);
        let result = tokio::time::timeout(Duration::from_secs(5), self.service)
            .await
            .unwrap()
            .unwrap();
        assert!(
            result.is_ok()
                || result
                    .as_ref()
                    .is_err_and(sdk::is_incoming_transport_closed)
        );
    }
}

#[derive(Clone, Copy)]
enum Loss {
    Admission,
    Decision,
}

struct Lost {
    inner: Arc<dyn Transport>,
    pending: AtomicBool,
    kind: Loss,
    record: Arc<std::sync::Mutex<Option<Request>>>,
}
impl Transport for Lost {
    fn target(&self) -> sailry_protocol::NodeId {
        self.inner.target()
    }
    fn subscribe(
        &self,
        topic: sailry_protocol::Topic,
    ) -> Pending<'_, std::result::Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(
        &self,
        request: sailry_protocol::Request,
    ) -> Pending<'_, std::result::Result<Admission, Fault>> {
        Box::pin(async move {
            let selected = match self.kind {
                Loss::Admission => matches!(request.command, Command::SubmitTurn { .. }),
                Loss::Decision => matches!(request.command, Command::ResolveApproval { .. }),
            };
            let lose = selected && self.pending.swap(false, Ordering::SeqCst);
            if lose {
                *self.record.lock().unwrap() = Some(request.clone());
            }
            let admitted = self.inner.dispatch(request).await?;
            if lose {
                assert!(admitted.receipt.durable);
                assert!(admitted.completion.await.unwrap().is_ok());
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "injected loss after Node admission",
                ));
            }
            Ok(admitted)
        })
    }
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
                id: ProviderId::new(),
                revision: 0,
                name: "ACP model".into(),
                api: conversation::ModelApi::ChatCompletions,
                authentication: Authentication::ApiKey,
                endpoint: endpoint.into(),
                enabled: true,
                models: vec![
                    conversation::discovery::Model {
                        id: "fixture-a".into(),
                        display_name: None,
                        context: Some(128_000),
                        output: Some(4096),
                        capabilities: None,
                    }
                    .configuration(
                        None,
                        conversation::ModelApi::ChatCompletions,
                        Authentication::ApiKey,
                    ),
                ],
                default_model: "fixture-a".into(),
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
