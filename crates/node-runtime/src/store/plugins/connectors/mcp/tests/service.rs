use super::*;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn package(client: &Client, name: &str) -> Info {
    let Output::Plugin(info) = execute(
        client,
        Command::InstallMcp {
            name: name.into(),
            expected_revision: 0,
            definition: plugin::mcp::Definition::StreamableHttp {
                url: "https://example.com/mcp".into(),
                headers: vec![],
            },
            secrets: BTreeMap::new(),
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    info
}

async fn connections(client: &Client) -> Vec<plugin::connector::Connection> {
    let Output::Connections(rows) = execute(client, Command::ListConnections).await else {
        panic!("connections expected")
    };
    rows
}

#[tokio::test]
async fn restores_only_its_account() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = crate::Node::start_empty(&profile).await.unwrap();
        let client = Client::new(node.local());
        let mut current = package(&client, "mcp-fixture").await;
        let other = package(&client, "mcp-other").await;
        let anonymous = package(&client, "mcp-anonymous").await;
        let mut unavailable = Vec::new();
        for name in ["mcp-expired", "mcp-missing", "mcp-pending", "mcp-consent"] {
            unavailable.push(package(&client, name).await);
        }
        drop(client);
        node.shutdown().await.unwrap();
        // Seed a stored account while the isolated fixture profile is closed.
        // No OAuth server or user profile is involved in the legacy combination.
        let db = Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let host = crate::plugins::Host::new(Some(profile.canonicalize().unwrap()));
        for info in [&current, &other].into_iter().chain(&unavailable) {
            let (mut record, _) = super::super::super::create(
                &db,
                host.profile(),
                &Command::StartConnectorLogin {
                    package: info.summary.reference(),
                    connector: "server".into(),
                    connection: None,
                    expected_revision: None,
                    method: Kind::McpOauth,
                    input: Input::default(),
                },
            )
            .unwrap();
            record.enabled = false;
            record.state = if info.summary.name == "mcp-pending" {
                State::Connecting
            } else {
                State::Connected
            };
            record.grant = (info.summary.name != "mcp-missing").then(|| Grant {
                access: Secret::new("private-retained-grant".into()),
                refresh: None,
                expires_at_ms: (info.summary.name == "mcp-expired").then_some(1),
                extra: BTreeMap::new(),
            });
            if info.summary.name == "mcp-consent" {
                record
                    .bound
                    .as_mut()
                    .unwrap()
                    .origins
                    .push("https://changed.example.com".into());
            }
            super::super::super::save(&db, &record).unwrap();
        }
        drop(db);
        let node = crate::Node::start_empty(&profile).await.unwrap();
        // Recovery cancels the interrupted login. Supply an active pending state
        // as isolated fixture input before observing its public projection.
        let db = Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let mut pending = records(&db)
            .unwrap()
            .into_iter()
            .find(|record| record.plugin == "mcp-pending")
            .unwrap();
        assert_eq!(pending.state, State::NotConnected);
        pending.state = State::Connecting;
        super::super::super::save(&db, &pending).unwrap();
        drop(db);
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
        let rows = connections(&client).await;
        let original = rows
            .iter()
            .find(|row| row.package.name == current.summary.name)
            .unwrap()
            .clone();
        let untouched = rows
            .iter()
            .find(|row| row.package.name == other.summary.name)
            .unwrap()
            .clone();
        let Output::Plugin(read) = execute(
            &client,
            Command::ReadPlugin {
                name: current.summary.name.clone(),
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        assert_eq!(read.summary, current.summary);
        assert_eq!(
            read.mcp[0].availability,
            plugin::mcp::Availability::ConnectionDisabled
        );
        current = read;
        for info in unavailable {
            use plugin::mcp::Availability;
            let (state, availability) = match info.summary.name.as_str() {
                "mcp-expired" => (State::Expired, Availability::AuthorizationExpired),
                "mcp-missing" => (State::NotConnected, Availability::ConnectionRequired),
                "mcp-pending" => (State::Connecting, Availability::ConnectionRequired),
                _ => (State::NeedsConfirmation, Availability::ConsentRequired),
            };
            let before = rows
                .iter()
                .find(|row| row.package.name == info.summary.name)
                .unwrap();
            assert_eq!(before.state, state);
            assert!(!before.enabled);
            let Output::Plugin(read) = execute(
                &client,
                Command::ReadPlugin {
                    name: info.summary.name.clone(),
                },
            )
            .await
            else {
                panic!("plugin expected")
            };
            assert_eq!(read.mcp[0].availability, availability);
            let Output::Plugin(enabled) = execute(
                &client,
                Command::SetPluginMcpAllowed {
                    package: read.summary.reference(),
                    server: "server".into(),
                    allowed: true,
                },
            )
            .await
            else {
                panic!("plugin expected")
            };
            assert!(enabled.mcp[0].allowed);
            assert_eq!(enabled.mcp[0].availability, availability);
            assert_eq!(
                connections(&client)
                    .await
                    .iter()
                    .find(|row| row.id == before.id)
                    .unwrap(),
                before
            );
        }
        let count = connections(&client).await.len();
        let Output::Plugin(anonymous) = execute(
            &client,
            Command::SetPluginMcpAllowed {
                package: anonymous.summary.reference(),
                server: "server".into(),
                allowed: true,
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        assert_eq!(
            anonymous.mcp[0].availability,
            plugin::mcp::Availability::Available
        );
        assert_eq!(connections(&client).await.len(), count);
        for allowed in [true, false] {
            if !allowed {
                let Output::Plugin(info) = execute(
                    &client,
                    Command::SetPluginMcpAllowed {
                        package: current.summary.reference(),
                        server: "server".into(),
                        allowed: false,
                    },
                )
                .await
                else {
                    panic!("plugin expected")
                };
                current = info;
                let row = connections(&client)
                    .await
                    .into_iter()
                    .find(|row| row.id == original.id)
                    .unwrap();
                execute(
                    &client,
                    Command::SetConnectionEnabled {
                        connection: row.id,
                        expected_revision: row.revision,
                        enabled: false,
                    },
                )
                .await;
            }
            let disabled = connections(&client)
                .await
                .into_iter()
                .find(|row| row.id == original.id)
                .unwrap();
            assert!(!disabled.enabled);
            let Output::Plugin(read) = execute(
                &client,
                Command::ReadPlugin {
                    name: current.summary.name.clone(),
                },
            )
            .await
            else {
                panic!("plugin expected")
            };
            assert_eq!(read, current);
            assert_eq!(
                connections(&client)
                    .await
                    .into_iter()
                    .find(|row| row.id == original.id)
                    .unwrap(),
                disabled
            );
            let stale = current.summary.reference();
            let request = client.prepare(Command::SetPluginMcpAllowed {
                package: stale.clone(),
                server: "server".into(),
                allowed: true,
            });
            let Output::Plugin(info) = client.execute(request.clone()).await.unwrap() else {
                panic!("plugin expected")
            };
            current = info;
            assert!(current.mcp[0].allowed && current.summary.enabled);
            assert!(current.summary.settings_revision > stale.settings_revision);
            assert_eq!(
                client.execute(request).await.unwrap(),
                Output::Plugin(current.clone())
            );
            assert_eq!(
                client
                    .execute(client.prepare(Command::SetPluginMcpAllowed {
                        package: stale,
                        server: "server".into(),
                        allowed: true
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
            let rows = connections(&client).await;
            let enabled = rows.iter().find(|row| row.id == original.id).unwrap();
            assert!(enabled.enabled && enabled.can_disconnect);
            assert_eq!(enabled.state, State::Connected);
            assert_eq!(
                rows.iter().find(|row| row.id == untouched.id).unwrap(),
                &untouched
            );
            assert!(
                !serde_json::to_string(&rows)
                    .unwrap()
                    .contains("private-retained")
            );
        }
        let captured = current.summary.reference();
        execute(
            &client,
            Command::RemovePlugin {
                name: current.summary.name,
                expected_revision: current.summary.revision,
            },
        )
        .await;
        let rows = connections(&client).await;
        assert!(!rows.iter().any(|row| row.id == original.id));
        assert_eq!(
            rows.iter().find(|row| row.id == untouched.id).unwrap(),
            &untouched
        );
        assert!(matches!(
            execute(&client, Command::ReadPluginVersion { package: captured }).await,
            Output::Plugin(_)
        ));
        drop(client);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
