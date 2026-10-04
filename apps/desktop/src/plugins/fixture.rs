use crate::conversation::live::Binding;
use sailry_client::Client;
use sailry_link::{Admission, CancellationToken, Pending, Subscription, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Fault, NodeId, Output, Request, Session, Topic, Update};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};

pub(crate) struct Observed {
    inner: Arc<dyn Transport>,
    terminal_failure: Mutex<Option<(Request, Fault)>>,
    pub mode: AtomicU8,
    pub requests: Mutex<Vec<Request>>,
    pub view_target: Mutex<Option<sailry_protocol::plugin::Reference>>,
    pub views: Mutex<Vec<Request>>,
    pub files: Arc<AtomicUsize>,
    pub entered: CancellationToken,
    pub release: CancellationToken,
}

impl Transport for Observed {
    fn open(
        &self,
        stream: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        self.inner.open(stream)
    }
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if let Command::ReadPluginView { package, .. } = &request.command
                && self.view_target.lock().unwrap().as_ref() == Some(package)
            {
                self.views.lock().unwrap().push(request.clone());
                if self
                    .mode
                    .compare_exchange(18, 0, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    return Err(Fault::new(
                        sailry_protocol::ErrorCode::Unavailable,
                        "injected plugin resource read failure",
                    ));
                }
                let busy = self.mode.load(Ordering::SeqCst) == 16
                    || self
                        .mode
                        .compare_exchange(15, 0, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok();
                if busy {
                    self.entered.cancel();
                    return Err(Fault::new(
                        sailry_protocol::ErrorCode::Busy,
                        "injected plugin resource capacity exhaustion",
                    ));
                }
            }
            if let Command::InspectRequest { id, digest } = &request.command {
                let failed = self.terminal_failure.lock().unwrap().clone();
                if let Some((original, error)) = failed.filter(|(original, _)| original.id == *id) {
                    assert_eq!(
                        digest,
                        &blake3::hash(&serde_json::to_vec(&original).unwrap())
                            .to_hex()
                            .to_string()
                    );
                    let (sender, completion) = tokio::sync::oneshot::channel();
                    sender
                        .send(Ok(Output::RequestOutcome {
                            id: *id,
                            outcome: sailry_protocol::RequestOutcome::Completed(Box::new(Err(
                                error,
                            ))),
                        }))
                        .unwrap();
                    return Ok(Admission {
                        receipt: sailry_protocol::Receipt {
                            id: request.id,
                            durable: false,
                        },
                        completion,
                    });
                }
                if self
                    .mode
                    .compare_exchange(10, 0, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    return Err(Fault::new(
                        sailry_protocol::ErrorCode::Unavailable,
                        "injected receipt inspection failure",
                    ));
                }
                if self
                    .mode
                    .compare_exchange(7, 0, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    let (sender, completion) = tokio::sync::oneshot::channel();
                    sender
                        .send(Ok(Output::RequestOutcome {
                            id: *id,
                            outcome: sailry_protocol::RequestOutcome::Admitted,
                        }))
                        .unwrap();
                    return Ok(Admission {
                        receipt: sailry_protocol::Receipt {
                            id: request.id,
                            durable: false,
                        },
                        completion,
                    });
                }
            }
            if request.plugin.is_some()
                || matches!(
                    request.command,
                    Command::CreateTerminal(_)
                        | Command::OpenToolTerminal { .. }
                        | Command::OpenTerminal { .. }
                        | Command::CloseTerminal { .. }
                        | Command::ClaimTerminal { .. }
                        | Command::InputTerminal { .. }
                        | Command::ResizeTerminal { .. }
                        | Command::SetTerminalAppearance { .. }
                        | Command::CreateProject(_)
                )
            {
                self.requests.lock().unwrap().push(request.clone());
            }
            if matches!(request.command, Command::OpenToolTerminal { .. })
                && self
                    .mode
                    .compare_exchange(13, 0, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                // Exercise confirmed SDK failure handling without allocating 32 PTYs.
                *self.terminal_failure.lock().unwrap() = Some((
                    request.clone(),
                    Fault::new(
                        sailry_protocol::ErrorCode::Busy,
                        "injected terminal capacity exhaustion",
                    ),
                ));
            }
            let failed = self.terminal_failure.lock().unwrap().clone();
            if let Some((original, error)) =
                failed.filter(|(original, _)| original.id == request.id)
            {
                assert_eq!(request, original);
                let (sender, completion) = tokio::sync::oneshot::channel();
                sender.send(Err(error)).unwrap();
                return Ok(Admission {
                    receipt: sailry_protocol::Receipt {
                        id: request.id,
                        durable: true,
                    },
                    completion,
                });
            }
            if matches!(request.command, Command::ReadPluginSettings { .. })
                && self.mode.load(Ordering::SeqCst) == 4
            {
                self.mode.store(0, Ordering::SeqCst);
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::Unavailable,
                    "injected plugin settings read failure",
                ));
            }
            if matches!(request.command, Command::CloseTerminal { .. })
                && self
                    .mode
                    .compare_exchange(11, 0, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                self.entered.cancel();
                self.release.cancelled().await;
            }
            if matches!(request.command, Command::ReadPluginView { .. })
                && self.mode.load(Ordering::SeqCst) == 2
            {
                self.mode.store(0, Ordering::SeqCst);
                self.entered.cancel();
                self.release.cancelled().await;
            }
            if matches!(request.command, Command::GeneratePluginText { .. })
                && self
                    .mode
                    .compare_exchange(8, 0, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                self.entered.cancel();
                self.release.cancelled().await;
            }
            let model_loss = matches!(request.command, Command::GeneratePluginText { .. })
                && (self
                    .mode
                    .compare_exchange(5, 0, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                    || self
                        .mode
                        .compare_exchange(6, 7, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok());
            let callback_loss = matches!(request.command, Command::CallPlugin { .. })
                && self
                    .mode
                    .compare_exchange(9, 10, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok();
            let terminal_loss = matches!(request.command, Command::OpenToolTerminal { .. })
                && self
                    .mode
                    .compare_exchange(12, 10, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok();
            let ssh_loss = matches!(request.command, Command::FinishSshUpload { .. })
                && self
                    .mode
                    .compare_exchange(14, 10, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok();
            let project_loss = matches!(request.command, Command::CreateProject(_))
                && self
                    .mode
                    .compare_exchange(17, 0, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok();
            let mode = if model_loss || callback_loss || terminal_loss || ssh_loss || project_loss {
                1
            } else if matches!(request.command, Command::WriteFile { .. }) {
                self.mode.swap(0, Ordering::SeqCst)
            } else {
                0
            };
            let admission = self.inner.dispatch(request).await?;
            if mode == 3 {
                self.entered.cancel();
                self.release.cancelled().await;
            }
            if mode != 1 {
                return Ok(admission);
            }
            admission.completion.await.unwrap().unwrap();
            Err(Fault::new(
                sailry_protocol::ErrorCode::Unavailable,
                "injected plugin receipt loss",
            ))
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            let files = matches!(topic, Topic::Files(_));
            let stream = self.inner.subscribe(topic).await?;
            if !files {
                return Ok(stream);
            }
            self.files.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Watching {
                inner: stream,
                count: self.files.clone(),
            }) as Box<dyn Subscription>)
        })
    }
}

struct Watching {
    inner: Box<dyn Subscription>,
    count: Arc<AtomicUsize>,
}
impl Subscription for Watching {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        self.inner.next()
    }
}
impl Drop for Watching {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) struct Fixture {
    pub directory: tempfile::TempDir,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub node: Node,
    pub controller: Node,
    pub transport: Arc<Observed>,
    pub binding: Binding,
    pub session: Session,
}

impl Fixture {
    pub fn new(remote: bool) -> Self {
        Self::with_git(remote, true)
    }

    pub fn directory(remote: bool) -> Self {
        Self::with_git(remote, false)
    }

    fn with_git(remote: bool, initialized: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(Node::start(directory.path().join("node")))
            .unwrap();
        let controller = runtime
            .block_on(Node::start(directory.path().join("controller")))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .link()
                    .pair(node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let inner = if remote {
            controller.link().remote(address)
        } else {
            node.local()
        };
        let transport = Arc::new(Observed {
            inner,
            terminal_failure: Mutex::new(None),
            mode: AtomicU8::new(0),
            requests: Mutex::new(Vec::new()),
            view_target: Mutex::new(None),
            views: Mutex::new(Vec::new()),
            files: Arc::new(AtomicUsize::new(0)),
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
        });
        let client = Arc::new(Client::new(transport.clone()));
        let execute = |command| {
            runtime
                .block_on(client.execute(client.prepare(command)))
                .unwrap()
        };
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        if initialized {
            git2::Repository::init(&root).unwrap();
        }
        std::fs::write(root.join("notes.txt"), "Complete 中文 🙂\n").unwrap();
        let Output::Project(project) = execute(Command::RegisterProject {
            name: "Plugin fixture".into(),
            path: root.to_str().unwrap().into(),
        }) else {
            panic!("project expected");
        };
        let provider = sailry_protocol::conversation::Provider {
            options: None,
            id: sailry_protocol::ProviderId::new(),
            revision: 0,
            name: "Unused fixture model".into(),
            api: sailry_protocol::conversation::ModelApi::ChatCompletions,
            authentication: sailry_protocol::Authentication::ApiKey,
            endpoint: "http://127.0.0.1:12345/v1".into(),
            enabled: true,
            credential: None,
            default_model: "fixture".into(),
            models: vec![sailry_protocol::conversation::Model {
                id: "fixture".into(),
                context: 4096,
                output: 128,
                vision: false,
                tools: true,
                reasoning: false,
                web_search: false,
                generates: vec![],
                efforts: Vec::new(),
                custom_efforts: false,
                default_effort: sailry_protocol::Effort::Default,
            }],
        };
        execute(Command::PutProvider {
            provider: provider.clone(),
            expected_revision: 0,
        });
        let Output::Session(session) = execute(Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(sailry_protocol::SessionConfig {
                assistant: None,
                resource: None,
                provider: provider.id,
                model: "fixture".into(),
                effort: sailry_protocol::Effort::Medium,
                mode: sailry_protocol::WorkMode::Code,
                permission: sailry_protocol::Permission::Ask,
                credential: None,
            }),
        }) else {
            panic!("session expected");
        };
        // Isolate the package under test from other shipped execution tools.
        for name in [
            "web-search",
            "goals",
            "commands",
            "browser",
            "context7",
            "github",
        ] {
            execute(Command::SetPluginEnabled {
                name: name.into(),
                expected_revision: 1,
                enabled: false,
            });
        }
        let binding = Binding {
            client: client.clone(),
            defaults: client,
            runtime: runtime.clone(),
            project: Some(project.id),
            worktree: Some(session.worktree),
            host: "Plugin Node".into(),
            project_name: "Plugin fixture".into(),
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
        }
    }

    pub fn package(&self) {
        let root = self.directory.path().join("project/package");
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/plugins/project-summary");
        copy_package(&source, &root);
    }

    pub fn install(&self, expected_revision: u64) -> sailry_protocol::plugin::Info {
        let Output::Plugin(info) = self.execute(Command::InstallPlugin {
            worktree: self.binding.worktree.unwrap(),
            path: "package".into(),
            name: "project-summary".into(),
            expected_revision,
        }) else {
            panic!("plugin expected");
        };
        info
    }

    pub fn configure(
        &self,
        package: &sailry_protocol::plugin::Info,
        include: bool,
    ) -> sailry_protocol::plugin::Info {
        self.execute(Command::SavePluginSettings {
            package: package.summary.reference(),
            values: std::collections::BTreeMap::from([(
                "include_untracked".into(),
                serde_json::json!(include),
            )]),
            secrets: std::collections::BTreeMap::new(),
        });
        let Output::Plugin(info) = self.execute(Command::ReadPlugin {
            name: package.summary.name.clone(),
        }) else {
            panic!("plugin expected")
        };
        info
    }

    #[track_caller]
    pub fn execute(&self, command: Command) -> Output {
        self.runtime
            .block_on(
                self.binding
                    .client
                    .execute(self.binding.client.prepare(command)),
            )
            .unwrap()
    }

    pub fn services(&self, remote: bool) -> crate::backend::Services {
        let node = if remote { &self.controller } else { &self.node };
        crate::backend::Services {
            runtime: self.runtime.clone(),
            local: node.local(),
            link: node.link(),
            relay_enabled: false,
        }
    }

    pub fn close(self) {
        self.runtime.block_on(self.node.shutdown()).unwrap();
        self.runtime.block_on(self.controller.shutdown()).unwrap();
    }
}

pub(crate) fn copy_package(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_package(&path, &target);
        } else {
            std::fs::copy(path, target).unwrap();
        }
    }
}
