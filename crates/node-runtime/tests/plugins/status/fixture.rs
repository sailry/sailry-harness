use super::*;

pub(super) struct Fixture {
    pub directory: tempfile::TempDir,
    pub node: Node,
    pub controller: Link,
    pub client: Arc<Client>,
    pub worktree: WorktreeId,
}

impl Fixture {
    pub async fn new() -> Self {
        let (directory, node, controller, client, worktree) = support::empty().await;
        Self {
            directory,
            node,
            controller,
            client: Arc::new(client),
            worktree,
        }
    }

    pub fn root(&self) -> std::path::PathBuf {
        self.directory.path().join("source")
    }

    pub fn declaration(&self, name: &str, changes: &[Change]) -> Value {
        json!({
            "$schema":plugin::schema::AGENT_PLUGINS_ID,"name":name,"version":"0.1.0",
            "extensions":{"dev.sailry.platform":{
                "api_version":"v1","scope":"host",
                "actions":["storage.read","storage.write","conversation.read","conversation.control","git.read","web.read"],
                "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
                    "handlers":["badge","invoke"],"read_only":["badge"],
                    "status":{"read":"badge","invoke":"invoke","changes":changes}},
                "ui":[
                    {"id":"badge","slot":"status","kind":"button","order":0,"label":{"label":"Badge"},"handler":"badge"},
                    {"id":"omitted","slot":"status","kind":"button","order":1,"label":{"label":"Omitted"},"handler":"omitted"}
                ]
            }}
        })
    }

    pub async fn install(
        &self,
        name: &str,
        source: &str,
        changes: &[Change],
        revision: u64,
    ) -> Info {
        self.install_with(name, source, self.declaration(name, changes), revision)
            .await
    }

    pub async fn install_with(
        &self,
        name: &str,
        source: &str,
        manifest: Value,
        revision: u64,
    ) -> Info {
        let root = self.root().join(name);
        fs::create_dir_all(root.join(plugin::NAMESPACE)).unwrap();
        fs::write(
            root.join("plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(root.join("dev.sailry.platform/main.js"), source).unwrap();
        fs::write(
            root.join("dev.sailry.platform/view.js"),
            "export default class View {}",
        )
        .unwrap();
        let package = info(
            execute(
                &self.client,
                Command::InstallPlugin {
                    worktree: self.worktree,
                    path: name.into(),
                    name: name.into(),
                    expected_revision: revision,
                    expected_digest: None,
                },
            )
            .await,
        );
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        assert!(package.summary.enabled);
        assert!(package.skills.is_empty() && package.mcp.is_empty());
        package
    }

    pub fn context(&self, package: &Info, scope: Scope) -> Context {
        Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Composer,
            package: package.summary.reference(),
            worktree: scope.worktree,
            session: scope.session,
        }
    }

    pub async fn read(&self, context: &Context, key: &str) -> plugin::storage::Entry {
        let Output::PluginValue(value) = self
            .client
            .execute(
                self.client
                    .prepare(Command::ReadPluginValue { key: key.into() })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("plugin value expected")
        };
        value
    }

    pub async fn write(&self, context: &Context, key: &str, value: Value, expected_revision: u64) {
        self.client
            .execute(
                self.client
                    .prepare(Command::WritePluginValue {
                        key: key.into(),
                        value,
                        expected_revision,
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap();
    }

    pub async fn session(&self, assistant: Option<plugin::conversation::Binding>) -> Session {
        let provider = ProviderId::new();
        let declaration: conversation::Provider = serde_json::from_value(json!({
            "id":provider,"revision":0,"name":"Status model fixture","api":"chat_completions",
            "authentication":"api_key","endpoint":"http://127.0.0.1:9/v1","enabled":true,
            "models":[],"default_model":"fixture","credential":null
        }))
        .unwrap();
        execute(
            &self.client,
            Command::PutProvider {
                provider: declaration,
                expected_revision: 0,
            },
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&self.client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|entry| entry.id == self.worktree)
            .unwrap()
            .project;
        let Output::Session(session) = execute(
            &self.client,
            Command::CreateSession {
                project,
                worktree: Some(self.worktree),
                config: Some(SessionConfig {
                    assistant,
                    resource: None,
                    provider,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    features: Default::default(),
                    mode: WorkMode::Code,
                    permission: Permission::Full,
                    options: Default::default(),
                    credential: None,
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        session
    }

    pub async fn close(self) {
        drop(self.client);
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

pub(super) struct Feed {
    pub updates: watch::Receiver<View>,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Feed {
    pub fn new(client: Arc<Client>, scope: Scope) -> Self {
        let (updates, receiver) = watch::channel(View::default());
        let stop = CancellationToken::new();
        let closed = stop.clone();
        let task = tokio::spawn(async move { client.watch_status(scope, updates, closed).await });
        Self {
            updates: receiver,
            stop,
            task: Some(task),
        }
    }

    pub fn current(&self) -> View {
        self.updates.borrow().clone()
    }

    pub async fn until(&mut self, accepts: impl Fn(&View) -> bool) -> View {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let value = self.updates.borrow_and_update().clone();
                if accepts(&value) {
                    return value;
                }
                self.updates
                    .changed()
                    .await
                    .expect("status owner is active");
            }
        })
        .await
        .expect("status update expected")
    }

    pub async fn close(mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.await.unwrap();
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn label(view: &View) -> Option<&str> {
    view.entries
        .iter()
        .find(|entry| entry.declaration.id == "badge" && entry.state.visible)
        .and_then(|entry| entry.state.label.as_ref())
        .map(|label| label.label.as_str())
}

pub(super) fn detail<'a>(view: &'a View, label: &str) -> Option<&'a str> {
    view.entries
        .iter()
        .find(|entry| entry.declaration.id == "badge")?
        .state
        .details
        .iter()
        .find(|detail| detail.label.label == label)
        .map(|detail| detail.value.as_str())
}
