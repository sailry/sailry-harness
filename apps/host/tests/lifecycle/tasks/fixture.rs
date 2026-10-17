use super::*;

pub(crate) struct Fixture {
    pub directory: tempfile::TempDir,
    pub profile: PathBuf,
    pub root: PathBuf,
    pub host: Host,
    pub client: Client,
    pub session: Session,
    pub model: models::Server,
    controller: Link,
}

impl Fixture {
    pub async fn new(model: models::Server) -> Self {
        let directory = tempdir().unwrap();
        let profile = directory.path().join("host");
        let root = directory.path().join("project 中文");
        std::fs::create_dir(&root).unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let prepared = Node::start_isolated(&profile).await.unwrap();
        controller
            .handle()
            .pair(prepared.link().invite().unwrap().ticket())
            .await
            .unwrap();
        prepared.shutdown().await.unwrap();
        let mut host = Host::spawn(&profile);
        host.ready();
        let client = Client::new(controller.handle().remote(host.address.clone().unwrap()));
        for name in ["context7", "github", "code-review"] {
            let Output::Plugin(info) =
                execute(&client, NodeCommand::ReadPlugin { name: name.into() }).await
            else {
                panic!("plugin expected")
            };
            if info.summary.enabled {
                execute(
                    &client,
                    NodeCommand::SetPluginEnabled {
                        name: name.into(),
                        expected_revision: info.summary.revision,
                        enabled: false,
                    },
                )
                .await;
            }
        }
        let Output::Provider(provider) = execute(
            &client,
            NodeCommand::PutProvider {
                expected_revision: 0,
                provider: Provider {
                    options: None,
                    oauth: None,
                    id: ProviderId::new(),
                    revision: 0,
                    name: "Management binary fixture".into(),
                    api: ModelApi::ChatCompletions,
                    authentication: Authentication::ApiKey,
                    endpoint: model.endpoint.clone(),
                    enabled: true,
                    models: Vec::new(),
                    default_model: String::new(),
                    credential: None,
                },
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let Output::Project(project) = execute(
            &client,
            NodeCommand::RegisterProject {
                name: "Management project".into(),
                path: root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let root = PathBuf::from(&project.path);
        let Output::Session(session) = execute(
            &client,
            NodeCommand::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    delegation_proactive: false,
                    permission: Permission::Ask,
                    credential: None,
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let Output::ProjectCatalog(catalog) =
            execute(&client, NodeCommand::ReadProjectCatalog).await
        else {
            panic!("project catalog expected")
        };
        let worktree = catalog
            .worktrees
            .iter()
            .find(|worktree| worktree.id == session.worktree)
            .expect("session worktree is registered");
        assert_eq!(worktree.project, Some(project.id));
        assert_eq!(Path::new(&worktree.path), root.as_path());
        assert_eq!(root, root.canonicalize().unwrap());
        Self {
            directory,
            profile,
            root,
            host,
            client,
            session,
            model,
            controller,
        }
    }

    pub fn submit(&self, request: RequestId, text: &str) -> Command {
        let mut command = self.submit_current(request, text);
        command.args(["--expected-revision", &self.session.revision.to_string()]);
        command
    }

    pub fn submit_current(&self, request: RequestId, text: &str) -> Command {
        let mut command = management(&self.profile, "submit");
        command.args([
            "--session",
            &self.session.id.to_string(),
            "--request-id",
            &request.to_string(),
            "--prompt",
            text,
            "--timeout",
            "10",
        ]);
        command
    }

    pub fn wait(&self, turn: TurnId, timeout: u64) -> Command {
        let mut command = management(&self.profile, "wait");
        command.args([
            "--session",
            &self.session.id.to_string(),
            "--turn",
            &turn.to_string(),
            "--timeout",
            &timeout.to_string(),
        ]);
        command
    }

    pub async fn invoke(&self, mut command: Command) -> std::process::Output {
        tokio::time::timeout(
            Duration::from_secs(20),
            tokio::task::spawn_blocking(move || command.output().unwrap()),
        )
        .await
        .unwrap()
        .unwrap()
    }

    pub async fn invoke_with_input(
        &self,
        mut command: Command,
        input: &str,
    ) -> std::process::Output {
        let input = input.to_owned();
        tokio::time::timeout(
            Duration::from_secs(20),
            tokio::task::spawn_blocking(move || {
                use std::io::Write;
                let mut child = command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let mut stdin = child.stdin.take().unwrap();
                stdin.write_all(input.as_bytes()).unwrap();
                drop(stdin);
                child.wait_with_output().unwrap()
            }),
        )
        .await
        .unwrap()
        .unwrap()
    }

    pub async fn execute(&self, command: NodeCommand) -> Output {
        execute(&self.client, command).await
    }

    pub async fn page(&self) -> conversation::Page {
        self.client
            .read_conversation(self.session.id, None, 100)
            .await
            .unwrap()
            .page
    }

    pub fn assert_identity(&self, value: &Value, request: RequestId) {
        assert_eq!(value["version"], 1);
        assert_eq!(
            value["node"],
            serde_json::to_value(self.client.target()).unwrap()
        );
        assert_eq!(value["request"], serde_json::to_value(request).unwrap());
        assert_eq!(
            value["session"],
            serde_json::to_value(self.session.id).unwrap()
        );
        assert_eq!(value["revision"], self.session.revision);
        assert!(!value["turn"].is_null());
    }

    pub async fn close(mut self) {
        if self.host.child.try_wait().unwrap().is_none() {
            self.host.stop();
        }
        self.controller.close().await.unwrap();
    }
}

async fn execute(client: &Client, command: NodeCommand) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

pub(super) fn binary() -> std::ffi::OsString {
    std::env::var_os("SAILRY_TEST_HOST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_sailry-host").into())
}

pub(super) fn management(profile: &Path, task: &str) -> Command {
    let mut command = Command::new(binary());
    command
        .arg(task)
        .arg("--data-dir")
        .arg(profile)
        .stdin(Stdio::null());
    command
}

pub(super) fn result(output: std::process::Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), 1, "{text}");
    serde_json::from_str(&text).unwrap()
}
