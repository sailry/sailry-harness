#![cfg(all(unix, feature = "test-support"))]
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    Command, ErrorCode, Output, Secret,
    plugin::{self, settings::SecretUpdate},
};
use std::{collections::BTreeMap, time::Duration};
#[path = "github/automatic.rs"]
mod automatic;
#[path = "support/github.rs"]
mod cli;

impl cli::Cli {
    fn set(&self, flag: &str) {
        std::fs::write(self.home().join(format!("gh-{flag}")), "").unwrap();
    }
    fn clear(&self, flag: &str) {
        std::fs::remove_file(self.home().join(format!("gh-{flag}"))).unwrap();
    }
    fn entered(&self) -> bool {
        self.home().join("gh-entered").exists()
    }
    fn environment(&self) -> String {
        std::fs::read_to_string(self.home().join("gh-environment")).unwrap()
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    cli: cli::Cli,
    node: Node,
    controller: Link,
    client: Client,
}
impl Fixture {
    async fn new(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let cli = cli::Cli::new(directory.path());
        let node = Node::start_empty_with_environment(directory.path().join("node"), cli.source())
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
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::InstallBundledPlugin {
                name: "github".into(),
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("GitHub plugin expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        Self {
            directory,
            cli,
            node,
            controller,
            client,
        }
    }
    async fn package(&self) -> plugin::Reference {
        let Output::Plugin(info) = self
            .client
            .execute(self.client.prepare(Command::ReadPlugin {
                name: "github".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("GitHub plugin expected")
        };
        info.summary.reference()
    }
    async fn state(&self) -> plugin::settings::State {
        let Output::PluginSettings(state) = self
            .client
            .execute(self.client.prepare(Command::ReadPluginSettings {
                package: self.package().await,
            }))
            .await
            .unwrap()
        else {
            panic!("plugin settings expected")
        };
        state
    }
    async fn close(self) {
        self.node.shutdown().await.unwrap();
        self.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn imports_explicitly_and_replays_without_token_output() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let mut events = fixture.client.subscribe().await.unwrap();
        assert!(
            !serde_json::to_string(&events.next().await.unwrap())
                .unwrap()
                .contains(cli::TOKEN)
        );
        let package = fixture.package().await;
        let status = fixture.client.prepare(Command::ReadGithubLogin {
            package: package.clone(),
        });
        assert!(!status.command.durable());
        let Output::GithubLogin(status) = fixture.client.execute(status).await.unwrap() else {
            panic!("CLI status expected")
        };
        assert!(status.available && status.authenticated);
        assert_eq!(status.account.as_deref(), Some("fixture-user"));
        assert!(!status.bound);
        assert!(!serde_json::to_string(&status).unwrap().contains(cli::TOKEN));
        assert_eq!(fixture.cli.calls("token"), 0);
        assert!(fixture.state().await.configured.is_empty());
        let request = fixture
            .client
            .prepare(Command::ImportGithubLogin { package });
        let admission = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let output = admission.completion.await.unwrap().unwrap();
        assert_eq!(fixture.client.execute(request).await.unwrap(), output);
        assert_eq!(fixture.cli.calls("token"), 1);
        assert!(!serde_json::to_string(&output).unwrap().contains(cli::TOKEN));
        let Output::PluginSettings(state) = output else {
            panic!("imported settings expected")
        };
        assert_eq!(state.configured, ["token"]);
        assert!(state.values.is_empty());
        assert_eq!(fixture.state().await, state);
        let Output::PluginSecret(secret) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPluginSecret {
                package: state.package.clone(),
                field: "token".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("secret slot expected")
        };
        assert!(secret.is_none());
        let database =
            rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                .unwrap();
        let bodies: Vec<Vec<u8>> = database
            .prepare("SELECT body FROM plugin_settings WHERE plugin='github'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            bodies
                .iter()
                .all(|body| !String::from_utf8_lossy(body).contains(cli::TOKEN))
        );
        drop(database);
        tokio::time::timeout(Duration::from_secs(5),async {
            loop {let event=events.next().await.unwrap();assert!(!serde_json::to_string(&event).unwrap().contains(cli::TOKEN));
                if matches!(event,sailry_protocol::Update::Event(ref envelope) if matches!(&envelope.event,sailry_protocol::Event::PluginChanged(summary) if summary.name=="github" && summary.settings_revision==state.package.settings_revision)) {break;}
            }
        }).await.unwrap();
        assert_eq!(
            fixture.cli.environment(),
            format!("{}:unset:unset\n", fixture.cli.home().display())
        );
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty_with_environment(&profile, fixture.cli.source())
            .await
            .unwrap();
        let address = fixture
            .controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(address)
        } else {
            node.local()
        });
        let Output::PluginSettings(restored) = client
            .execute(client.prepare(Command::ReadPluginSettings {
                package: state.package,
            }))
            .await
            .unwrap()
        else {
            panic!("restored settings expected")
        };
        assert_eq!(restored.configured, ["token"]);
        assert!(
            !serde_json::to_string(&restored)
                .unwrap()
                .contains(cli::TOKEN)
        );
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn refuses_stale_and_changed_targets() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let stale = fixture.package().await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::SavePluginSettings {
                package: stale.clone(),
                values: BTreeMap::new(),
                secrets: BTreeMap::from([(
                    "token".into(),
                    SecretUpdate::Replace(Secret::new("previous-token".into())),
                )]),
            }))
            .await
            .unwrap();
        let error = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::ImportGithubLogin { package: stale }),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        assert_eq!(fixture.cli.calls("token"), 0);
        fixture.cli.set("hold");
        let package = fixture.package().await;
        let request = fixture.client.prepare(Command::ImportGithubLogin {
            package: package.clone(),
        });
        let admission = fixture.client.dispatch(request.clone()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !fixture.cli.entered() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::SavePluginSettings {
                package,
                values: BTreeMap::new(),
                secrets: BTreeMap::from([("token".into(), SecretUpdate::Clear)]),
            }))
            .await
            .unwrap();
        fixture.cli.set("release");
        let error = admission.completion.await.unwrap().unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        assert_eq!(fixture.client.execute(request).await.unwrap_err(), error);
        assert!(fixture.state().await.configured.is_empty());
        assert_eq!(fixture.cli.calls("token"), 1);
        let root = fixture.directory.path().join("project");
        std::fs::create_dir_all(root.join("github/dev.sailry.platform")).unwrap();
        let Output::Project(project) = fixture
            .client
            .execute(fixture.client.prepare(Command::RegisterProject {
                name: "Fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.project == Some(project.id))
            .unwrap()
            .id;
        std::fs::write(root.join("github/plugin.json"),serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"github","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":[],"settings_schema":"dev.sailry.platform/settings.json","credential_source":"github_cli"}}}).to_string()).unwrap();
        std::fs::write(
            root.join("github/dev.sailry.platform/settings.json"),
            include_str!("../../../plugins/github/dev.sailry.platform/settings.json"),
        )
        .unwrap();
        std::fs::write(root.join("github/mcp.json"),serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"github":{"type":"streamable-http","url":"https://changed.invalid/mcp/","headers":{"Authorization":""}}}}).to_string()).unwrap();
        let Output::Plugin(current) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPlugin {
                name: "github".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("current plugin expected")
        };
        let Output::Plugin(changed) = fixture
            .client
            .execute(fixture.client.prepare(Command::InstallPlugin {
                worktree,
                path: "github".into(),
                name: "github".into(),
                expected_revision: current.summary.revision,
                expected_digest: None,
            }))
            .await
            .unwrap()
        else {
            panic!("changed plugin expected")
        };
        assert!(
            changed.settings.as_ref().unwrap().properties["token"]
                .secret
                .is_some()
        );
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::ImportGithubLogin {
                package: changed.summary.reference(),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(fixture.cli.calls("token"), 1);
        fixture.close().await;
    }
}

#[tokio::test]
async fn discards_status_and_failure_streams() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        fixture.cli.set("signed-out");
        let Output::GithubLogin(status) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadGithubLogin {
                package: fixture.package().await,
            }))
            .await
            .unwrap()
        else {
            panic!("status expected")
        };
        assert!(status.available && !status.authenticated);
        assert!(status.account.is_none());
        assert!(!status.bound);
        assert_eq!(fixture.cli.calls("token"), 0);
        fixture.cli.clear("signed-out");
        fixture.cli.set("fail");
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::ImportGithubLogin {
                package: fixture.package().await,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(!serde_json::to_string(&error).unwrap().contains(cli::TOKEN));
        fixture.cli.clear("fail");
        fixture.cli.set("oversized");
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::ImportGithubLogin {
                package: fixture.package().await,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(fixture.state().await.configured.is_empty());
        std::fs::remove_file(
            fixture
                .cli
                .source()
                .home
                .parent()
                .unwrap()
                .join("first-programs/gh"),
        )
        .unwrap();
        let Output::GithubLogin(status) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadGithubLogin {
                package: fixture.package().await,
            }))
            .await
            .unwrap()
        else {
            panic!("status expected")
        };
        assert!(!status.available && !status.authenticated);
        fixture.close().await;
    }
}

#[tokio::test]
async fn rolls_back_when_completion_cannot_be_recorded() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let before = fixture.state().await;
        let database =
            rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                .unwrap();
        database.execute_batch("CREATE TRIGGER reject_import BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'injected completion failure'); END;").unwrap();
        let request = fixture.client.prepare(Command::ImportGithubLogin {
            package: before.package.clone(),
        });
        let error = fixture.client.execute(request.clone()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::OutcomeUnknown);
        assert_eq!(fixture.cli.calls("token"), 1);
        assert_eq!(fixture.state().await, before);
        let rows: i64 = database
            .query_row(
                "SELECT count(*) FROM plugin_settings WHERE plugin='github'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0);
        database
            .execute_batch("DROP TRIGGER reject_import")
            .unwrap();
        drop(database);
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty_with_environment(profile, fixture.cli.source())
            .await
            .unwrap();
        let address = fixture
            .controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(address)
        } else {
            node.local()
        });
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(fixture.cli.calls("token"), 1);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn invalidates_links_after_logout_and_account_switch() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let Output::PluginSettings(linked) = fixture
            .client
            .execute(fixture.client.prepare(Command::ImportGithubLogin {
                package: fixture.package().await,
            }))
            .await
            .unwrap()
        else {
            panic!("linked settings expected")
        };
        for (flag, authenticated, bound) in [
            (None, true, true),
            (Some("signed-out"), false, false),
            (Some("switched"), true, false),
        ] {
            if let Some(flag) = flag {
                fixture.cli.set(flag);
            }
            let Output::GithubLogin(status) = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadGithubLogin {
                    package: linked.package.clone(),
                }))
                .await
                .unwrap()
            else {
                panic!("CLI status expected")
            };
            assert_eq!(status.authenticated, authenticated);
            assert_eq!(status.bound, bound);
            assert_eq!(fixture.cli.calls("token"), 1);
            assert!(!serde_json::to_string(&status).unwrap().contains(cli::TOKEN));
            if let Some(flag) = flag {
                fixture.cli.clear(flag);
            }
        }
        fixture.cli.set("signed-out");
        let Output::PluginSettings(manual) = fixture
            .client
            .execute(fixture.client.prepare(Command::SavePluginSettings {
                package: linked.package,
                values: linked.values,
                secrets: BTreeMap::from([(
                    "token".into(),
                    SecretUpdate::Replace(Secret::new("explicit-manual-token".into())),
                )]),
            }))
            .await
            .unwrap()
        else {
            panic!("manual settings expected")
        };
        let Output::PluginSecret(secret) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPluginSecret {
                package: manual.package.clone(),
                field: "token".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("manual secret expected")
        };
        assert_eq!(secret.unwrap().expose(), "explicit-manual-token");
        assert_eq!(manual.configured, ["token"]);
        fixture.close().await;
    }
}
