#![cfg(all(unix, feature = "test-support"))]
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output, plugin};
use std::time::Duration;
#[path = "support/github.rs"]
mod cli;

impl cli::Cli {
    fn set(&self, flag: &str) {
        std::fs::write(self.home().join(format!("gh-{flag}")), "").unwrap();
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
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
            _directory: directory,
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
    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn inspects_without_tokens_and_binds_identity() {
    use sailry_protocol::{
        Update,
        plugin::connector::{Input, Kind, LoginState},
    };
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let package = fixture.package().await;
        let Output::ConnectorIdentity(identity) = fixture
            .client
            .execute(fixture.client.prepare(Command::InspectConnectorCli {
                package: package.clone(),
                connector: "github".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("account expected")
        };
        assert_eq!(identity.id, "fixture-user");
        assert_eq!(fixture.cli.calls("token"), 0);
        let Output::Connections(rows) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListConnections))
            .await
            .unwrap()
        else {
            panic!("connections expected")
        };
        assert!(rows.is_empty());
        let Output::ConnectorLogin(attempt) = fixture
            .client
            .execute(fixture.client.prepare(Command::StartConnectorLogin {
                package,
                connector: "github".into(),
                connection: None,
                expected_revision: None,
                method: Kind::LocalCli,
                input: Input::default(),
            }))
            .await
            .unwrap()
        else {
            panic!("attempt expected")
        };
        let mut stream = fixture
            .client
            .subscribe_connector_login(attempt.id)
            .await
            .unwrap();
        let row = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let Update::ConnectorLogin(update) = stream.next().await.unwrap() else {
                    panic!("login expected")
                };
                assert!(!serde_json::to_string(&update).unwrap().contains(cli::TOKEN));
                if let LoginState::Connected(row) = update.state {
                    return row;
                }
                assert!(update.state.active(), "{:?}", update.state);
            }
        })
        .await
        .unwrap();
        assert_eq!(row.identity.as_ref().unwrap().id, "fixture-user");
        assert_eq!(fixture.cli.calls("token"), 1);
        fixture.cli.set("switched");
        assert!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::ProbeConnection { connection: row.id })
                )
                .await
                .is_err()
        );
        assert_eq!(fixture.cli.calls("token"), 1);
        let Output::Connections(rows) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListConnections))
            .await
            .unwrap()
        else {
            panic!("connections expected")
        };
        assert_ne!(
            rows[0].state,
            sailry_protocol::plugin::connector::State::Connected
        );
        assert!(!serde_json::to_string(&rows).unwrap().contains(cli::TOKEN));
        fixture.close().await;
    }
}
