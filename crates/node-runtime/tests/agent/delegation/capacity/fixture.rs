use super::*;

pub(super) struct Fixture {
    directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    pub client: Client,
    pub parents: Vec<Session>,
    pub provider: Provider,
    pub role: role::Profile,
    package: sailry_protocol::plugin::Info,
    pub parent: Server,
    pub child: Server,
    remote: bool,
}

impl Fixture {
    pub async fn start(remote: bool, background: bool, child: Server, tasks: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let tool = plugin_tool("example", "spawn");
        let parent = Server::batch(
            (0..tasks)
                .map(|index| {
                    (
                        tool.clone(),
                        json!({"role":"review","task":format!("Task {index}"),"background":background}),
                    )
                })
                .collect(),
        )
        .await;
        let (mut session, provider) = configured_session(&client, &parent.endpoint, &root).await;
        let mut provider = provider;
        provider.models = vec![model("fixture-a")];
        provider.default_model = "fixture-a".into();
        client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: provider.revision,
            }))
            .await
            .unwrap();
        let source = root.join("capacity-package");
        std::fs::create_dir_all(source.join("dev.sailry.platform")).unwrap();
        std::fs::write(source.join("dev.sailry.platform/main.js"), "export function prepare(input){return input} export function result({output}){return output.data??output}").unwrap();
        std::fs::write(source.join("dev.sailry.platform/settings.json"),json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,
            "required":["child_concurrency"],"properties":{"child_concurrency":{"type":"string","default":"8","x-sailry-child-concurrency":true}}
        }).to_string()).unwrap();
        std::fs::write(source.join("plugin.json"),json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":"example","version":"0.1.0",
            "extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["agents.delegate"],
                "settings_schema":"dev.sailry.platform/settings.json",
                "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"handlers":["prepare","result"]},
                "tools":[{"name":"spawn","description":"Run an independent fixture task","contexts":["workspace"],
                    "handler":{"name":"prepare","operation":"agents.delegate","result":"result","parameters":{
                        "type":"object","required":["task"],"additionalProperties":false,"properties":{
                            "task":{"type":"string"},"role":{"type":["string","null"]},"background":{"type":"boolean"}
                        }
                    }}
                }, {"name":"wait","description":"Observe an admitted fixture child","operation":"agents.wait","contexts":["workspace"]}]
            }}
        }).to_string()).unwrap();
        let Output::Plugin(info) = client
            .execute(client.prepare(Command::InstallPlugin {
                worktree: session.worktree,
                path: "capacity-package".into(),
                name: "example".into(),
                expected_revision: 0,
                expected_digest: None,
            }))
            .await
            .unwrap()
        else {
            panic!("package expected")
        };
        assert!(info.issues.is_empty(), "{:?}", info.issues);
        provider.id = ProviderId::new();
        provider.revision = 0;
        provider.endpoint = child.endpoint.clone();
        provider.models = vec![model("fixture-child")];
        provider.default_model = "fixture-child".into();
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let role = role::Profile {
            id: RoleId::new(),
            revision: 0,
            key: "review".into(),
            name: "Review".into(),
            description: "Inspect fixture scope".into(),
            instructions: "Frozen capacity instructions".into(),
            read_only: false,
            appearance: None,
            skills: vec![],
            max_turns: None,
            model: Some(role::Model {
                provider: provider.id,
                model: "fixture-child".into(),
                effort: None,
            }),
        };
        let Output::Role(role) = client
            .execute(client.prepare(Command::PutRole {
                role,
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("role expected")
        };
        session.config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config: session.config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let first = roster(&client, session, &role).await;
        let Output::Session(second) = client
            .execute(client.prepare(Command::CreateSession {
                project: first.project,
                worktree: Some(first.worktree),
                config: Some(first.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let second = roster(&client, second, &role).await;
        Self {
            directory,
            node,
            controller,
            client,
            parents: vec![first, second],
            provider,
            role,
            package: info,
            parent,
            child,
            remote,
        }
    }

    pub fn observation(&self, window: Duration) {
        self.node.set_child_observation_fixture(window);
    }

    pub async fn waiter(&self, child: &Run) -> Server {
        let server = Server::turn_tools(vec![(
            plugin_tool("example", "wait"),
            json!({"session":child.session,"turn":child.turn}),
        )])
        .await;
        let Output::Providers(providers) = self
            .client
            .execute(self.client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers
            .into_iter()
            .find(|provider| provider.id == self.parents[0].config.provider)
            .unwrap();
        provider.endpoint = server.endpoint.clone();
        self.client
            .execute(self.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        server
    }

    pub async fn capacity(&self, count: u64) {
        super::super::capacity(&self.client, "example", count).await;
    }

    pub async fn upgrade(&mut self) {
        let path = self
            .directory
            .path()
            .join("project/capacity-package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["version"] = json!("0.1.1");
        std::fs::write(path, manifest.to_string()).unwrap();
        let Output::Plugin(current) = self
            .client
            .execute(self.client.prepare(Command::ReadPlugin {
                name: "example".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("package expected")
        };
        self.package = current;
        let previous = self.package.summary.reference();
        let Output::Plugin(package) = self
            .client
            .execute(self.client.prepare(Command::InstallPlugin {
                worktree: self.parents[0].worktree,
                path: "capacity-package".into(),
                name: "example".into(),
                expected_revision: self.package.summary.revision,
                expected_digest: None,
            }))
            .await
            .unwrap()
        else {
            panic!("package expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        assert_ne!(package.summary.reference(), previous);
        self.package = package;
    }

    pub async fn push(&self, index: usize) -> QueuedTurn {
        let session = &self.parents[index];
        let Output::QueuedTurn(turn) = self
            .client
            .execute(self.client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Perform the independent fixture tasks".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        turn
    }

    pub async fn runs(&self) -> Vec<Run> {
        let mut runs = Vec::new();
        for parent in &self.parents {
            runs.extend(
                history(&self.client, parent.id)
                    .await
                    .children
                    .into_iter()
                    .map(|child| child.run),
            );
        }
        runs.sort_by_key(|run| run.sequence);
        runs
    }

    pub async fn wait(&self, total: usize, running: usize) -> Vec<Run> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let runs = self.runs().await;
                assert!(
                    !runs.iter().any(|run| run.status == Status::Failed),
                    "child failed before scheduling: {:?}",
                    runs.iter()
                        .map(|run| (run.status, &run.error))
                        .collect::<Vec<_>>()
                );
                if runs.len() == total
                    && runs
                        .iter()
                        .filter(|run| run.status == Status::Running)
                        .count()
                        == running
                {
                    return runs;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("child scheduling deadline")
    }

    pub async fn restart(mut self) -> Self {
        let id = self.node.id();
        self.node.shutdown().await.unwrap();
        self.controller.handle().disconnect(id).await;
        self.node = Node::start_empty(self.directory.path().join("node"))
            .await
            .unwrap();
        self.client = Client::new(if self.remote {
            self.controller.handle().remote(self.node.link().address())
        } else {
            self.node.local()
        });
        self
    }

    pub async fn close(self) {
        self.node.shutdown().await.unwrap();
        self.controller.close().await.unwrap();
    }
}

async fn roster(client: &Client, session: Session, role: &role::Profile) -> Session {
    let Output::Session(session) = client
        .execute(client.prepare(Command::SetSessionRoles {
            session: session.id,
            expected_revision: session.revision,
            roles: vec![role.reference()],
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}
