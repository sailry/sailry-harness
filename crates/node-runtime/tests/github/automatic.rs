use super::*;
use crate::agent_support;
use sailry_protocol::{
    Authentication, Effort, Permission, ProviderId, Session, SessionConfig, WorkMode,
    conversation::{ModelApi, Page, Provider, Status},
    plugin::mcp::{Configuration, Server},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

impl Fixture {
    async fn configure_mcp(
        &self,
        package: plugin::Reference,
        configuration: Configuration,
    ) -> plugin::Reference {
        let Output::PluginSettings(settings) = self
            .client
            .execute(self.client.prepare(Command::SavePluginSettings {
                package,
                values: BTreeMap::new(),
                secrets: BTreeMap::from([("token".into(), SecretUpdate::Keep)]),
            }))
            .await
            .unwrap()
        else {
            panic!("plugin settings expected")
        };
        // Bound settings prohibit public raw MCP edits. Inject only this fresh
        // isolated revision before capture; never rewrite an admitted revision.
        let database =
            rusqlite::Connection::open(self.node.profile().join("storage/node.sqlite3")).unwrap();
        database.busy_timeout(Duration::from_secs(5)).unwrap();
        let revision = i64::try_from(settings.package.settings_revision).unwrap();
        let body: Vec<u8> = database
            .query_row(
                "SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2",
                rusqlite::params![settings.package.name, revision],
                |row| row.get(0),
            )
            .unwrap();
        let mut stored: serde_json::Value = serde_json::from_slice(&body).unwrap();
        stored["mcp"] = serde_json::to_value(&configuration).unwrap();
        assert_eq!(
            database
                .execute(
                    "UPDATE plugin_settings SET body=?3 WHERE plugin=?1 AND revision=?2",
                    rusqlite::params![
                        settings.package.name,
                        revision,
                        serde_json::to_vec(&stored).unwrap()
                    ],
                )
                .unwrap(),
            1
        );
        drop(database);
        let Output::PluginMcp(saved) = self
            .client
            .execute(self.client.prepare(Command::ReadPluginMcp {
                package: settings.package.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("MCP configuration expected")
        };
        assert_eq!(saved.configuration, configuration);
        settings.package
    }

    async fn session(&self, endpoint: &str) -> Session {
        let Output::Plugins(packages) = self
            .client
            .execute(self.client.prepare(Command::ListPlugins))
            .await
            .unwrap()
        else {
            panic!("plugin inventory expected")
        };
        for package in packages {
            let enabled = package.name == "github";
            if package.enabled != enabled {
                self.client
                    .execute(self.client.prepare(Command::SetPluginEnabled {
                        name: package.name,
                        expected_revision: package.revision,
                        enabled,
                    }))
                    .await
                    .unwrap();
            }
        }
        let Output::Provider(provider) = self
            .client
            .execute(self.client.prepare(Command::PutProvider {
                provider: Provider {
                    preset: None,
                    binding: None,
                    consent: None,
                    id: ProviderId::new(),
                    revision: 0,
                    name: "Model fixture".into(),
                    api: ModelApi::ChatCompletions,
                    authentication: Authentication::ApiKey,
                    endpoint: endpoint.into(),
                    enabled: true,
                    models: Vec::new(),
                    default_model: String::new(),
                    credential: None,
                    options: None,
                    oauth: None,
                },
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let root = self.directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = self
            .client
            .execute(self.client.prepare(Command::RegisterProject {
                name: "GitHub fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(session) = self
            .client
            .execute(self.client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    model: "fixture-a".into(),
                    effort: Effort::High,
                    mode: WorkMode::Code,
                    delegation_proactive: false,
                    permission: Permission::Full,
                    credential: None,
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        session
    }

    async fn finished(&self, session: &Session, turn: sailry_protocol::TurnId) -> Page {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let Output::Conversation(history) = self
                    .client
                    .execute(self.client.prepare(Command::ReadConversation {
                        session: session.id,
                        before: None,
                        limit: 100,
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("conversation expected")
                };
                if history.page.runs.iter().any(|run| {
                    run.turn == turn
                        && !matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        )
                }) {
                    return history.page;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Agent completion deadline")
    }
}

fn assert_redacted(value: &impl serde::Serialize) {
    assert!(!serde_json::to_string(value).unwrap().contains(cli::TOKEN));
}

#[tokio::test]
async fn rejects_changed_endpoint_without_persisting_binding() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let model = agent_support::Server::markdown("Fixture answer".into()).await;
        let session = fixture.session(&model.endpoint).await;
        let Output::PluginMcp(initial) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPluginMcp {
                package: fixture.package().await,
            }))
            .await
            .unwrap()
        else {
            panic!("MCP configuration expected")
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp/", listener.local_addr().unwrap());
        let contacted = Arc::new(AtomicBool::new(false));
        let observer = {
            let contacted = contacted.clone();
            tokio::spawn(async move {
                if listener.accept().await.is_ok() {
                    contacted.store(true, Ordering::SeqCst);
                }
            })
        };
        let mut configuration = initial.configuration.clone();
        let Server::Http { url, .. } = configuration.servers.get_mut("github").unwrap() else {
            panic!("GitHub HTTP transport expected")
        };
        *url = endpoint.clone();
        let changed = fixture.configure_mcp(initial.package, configuration).await;
        assert_eq!(fixture.cli.calls("token"), 0);
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Reply to this isolated task".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        assert!(turn.plugins.contains(&changed));
        assert_redacted(&turn);
        let page = fixture.finished(&session, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(fixture.cli.calls("token"), 1);
        assert!(!contacted.load(Ordering::SeqCst));
        observer.abort();
        let requests = model.requests.lock().unwrap().clone();
        assert!(!requests.is_empty());
        let input = serde_json::to_string(&requests).unwrap();
        assert!(input.contains("MCP server github/github is unavailable"));
        assert!(!input.contains(&endpoint));
        assert_redacted(&requests);
        assert_redacted(&page);
        let settings = fixture.state().await;
        assert_eq!(settings.package, changed);
        assert!(settings.configured.is_empty());
        assert!(settings.values.is_empty());
        assert_redacted(&settings);
        let snapshot = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap();
        assert!(matches!(snapshot, Output::Snapshot(_)));
        assert_redacted(&snapshot);
        let database =
            rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                .unwrap();
        let bodies = database
            .prepare("SELECT body FROM plugin_settings WHERE plugin='github'")
            .unwrap()
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!bodies.is_empty());
        for body in bodies {
            let stored: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(stored["github"].is_null());
            assert!(stored["slots"].get("token").is_none());
            assert_redacted(&stored);
        }
        drop(database);
        fixture
            .configure_mcp(settings.package, initial.configuration)
            .await;
        let Output::GithubLogin(status) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadGithubLogin {
                package: fixture.package().await,
            }))
            .await
            .unwrap()
        else {
            panic!("CLI status expected")
        };
        assert!(status.available && status.authenticated);
        assert_eq!(status.account.as_deref(), Some("fixture-user"));
        assert!(!status.bound);
        assert_eq!(fixture.cli.calls("token"), 1);
        assert_redacted(&status);
        fixture.close().await;
    }
}
