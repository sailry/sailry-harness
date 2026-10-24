//! Interrupted attempts retain private baselines and preserve newer account edits.
use super::*;
use sailry_protocol::plugin::{Context, connector::State, http};
use serde_json::Value;

async fn begin(client: &Client, row: &plugin::connector::Connection, changed: bool) -> Attempt {
    let mut input = Input {
        redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
        ..Default::default()
    };
    if changed {
        input.secrets.insert(
            "client_secret".into(),
            Secret::new("pending-client-marker".into()),
        );
    }
    let output = execute(
        client,
        Command::StartConnectorLogin {
            package: row.package.clone(),
            connector: row.connector.clone(),
            connection: Some(row.id),
            expected_revision: Some(row.revision),
            method: row.method,
            input,
        },
    )
    .await;
    public(&output);
    let Output::ConnectorLogin(attempt) = output else {
        panic!("connector attempt expected")
    };
    state(client, &attempt, |state| {
        matches!(state, LoginState::Browser { .. })
    })
    .await;
    attempt
}

fn public(value: &impl serde::Serialize) {
    let value = serde_json::to_string(value).unwrap();
    assert!(!value.contains("\"previous\""));
    for secret in [
        "private-access-marker",
        "private-refresh-marker",
        "private-client-marker",
        "pending-client-marker",
        "replacement-client-marker",
    ] {
        assert!(!value.contains(secret));
    }
}

fn protected(
    package: plugin::Reference,
    worktree: WorktreeId,
    id: plugin::connector::Id,
) -> (Command, Context) {
    (
        Command::RequestPluginHttp(http::Request {
            url: "https://service.example.com/ok".into(),
            method: "GET".into(),
            headers: BTreeMap::new(),
            body: None,
            credential: None,
            connector: Some("account".into()),
            connection: Some(id),
            idempotent: false,
            timeout_ms: 2000,
        }),
        Context {
            package,
            worktree: Some(worktree),
            session: None,
            turn: None,
            invocation: None,
            surface: Default::default(),
        },
    )
}

fn configure_label(root: &Path) {
    configure(root);
    let filename = root.join("plugin.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&filename).unwrap()).unwrap();
    value["extensions"]["dev.sailry.platform"]["connectors"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"label","title":"Label"}));
    fs::write(filename, value.to_string()).unwrap();
}

#[tokio::test]
async fn recovers_interrupted_accounts_on_restart() {
    for remote in [false, true] {
        for changed in [false, true] {
            let issuer = server::Server::start(false, false).await;
            let resource = super::super::http::server::Server::start().await;
            let (directory, node, controller, client, worktree) = support::empty(remote).await;
            node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
            node.set_connector_fixture("https://service.example.com", &resource.origin);
            configure_label(&directory.path().join("source/package"));
            let package = info(execute(&client, install(worktree, 0)).await)
                .summary
                .reference();
            let original = lifecycle::connect(&client, package.clone()).await;
            let before: Value =
                serde_json::from_slice(&lifecycle::stored(&node, original.id).await).unwrap();
            let attempt = begin(&client, &original, changed).await;
            let interrupted = lifecycle::stored(&node, original.id).await;
            let pending: Value = serde_json::from_slice(&interrupted).unwrap();
            assert_eq!(
                pending["login"]["attempt"],
                serde_json::to_value(attempt.id).unwrap()
            );
            assert_eq!(pending["login"]["previous"]["grant"], before["grant"]);
            let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
                panic!("connections expected")
            };
            public(&rows);
            assert_eq!(rows[0].state, State::Connecting);
            let profile = node.profile().to_owned();
            let node_id = node.id();
            node.shutdown().await.unwrap();
            controller.handle().disconnect(node_id).await;
            drop(client);
            // Restore the exact durable in-progress bytes after orderly fixture
            // shutdown to model an interruption before the callback completed.
            let (database_handle, database) = crate::plugins::database_fixture::open(
                &(profile.join("storage/node.turso")),
                false,
            )
            .await;
            database
                .execute(
                    "UPDATE plugin_connections SET body=?1 WHERE id=?2",
                    turso::params![interrupted.as_slice(), original.id.to_string()],
                )
                .await
                .unwrap();
            let persisted: Vec<u8> = (async {
                let mut statement = database
                    .prepare("SELECT body FROM plugin_connections WHERE id=?1")
                    .await?;
                let row = statement.query_row([original.id.to_string()]).await?;
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                mapped
            })
            .await
            .unwrap();
            assert_eq!(persisted, interrupted);
            let orphaned: Value = serde_json::from_slice(&persisted).unwrap();
            assert_eq!(orphaned["state"], "connecting");
            assert_eq!(
                orphaned["login"]["attempt"],
                serde_json::to_value(attempt.id).unwrap()
            );
            crate::plugins::database_fixture::close(database_handle, database).await;
            let node = Node::start_empty(&profile).await.unwrap();
            node.set_connector_fixture("https://service.example.com", &resource.origin);
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
                panic!("connections expected")
            };
            public(&rows);
            assert_eq!(rows[0].state, State::Connected);
            assert_eq!(rows[0].revision, attempt.revision);
            let recovered: Value =
                serde_json::from_slice(&lifecycle::stored(&node, original.id).await).unwrap();
            assert!(recovered["login"].is_null());
            for field in [
                "fields",
                "secrets",
                "grant",
                "identity",
                "bound",
                "checked_at_ms",
            ] {
                assert_eq!(
                    recovered[field], before[field],
                    "{remote}/{changed}/{field}"
                );
            }
            for field in ["revision", "auth_revision", "grant_revision"] {
                assert_eq!(
                    recovered[field], pending[field],
                    "{remote}/{changed}/{field}"
                );
            }
            let (command, scope) = protected(package, worktree, original.id);
            assert!(
                client
                    .execute(client.prepare(command).with_plugin(scope))
                    .await
                    .is_ok()
            );
            assert_eq!(resource.requests.lock().unwrap().len(), 1);
            assert!(
                resource.requests.lock().unwrap()[0]
                    .0
                    .contains("Bearer private-access-marker")
            );
            assert_eq!(issuer.bodies.lock().unwrap().len(), 1);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn finishes_conflicts_without_overwriting_newer_edits() {
    for remote in [false, true] {
        for change in ["label", "disconnect", "secret"] {
            let issuer = server::Server::start(false, false).await;
            let resource = super::super::http::server::Server::start().await;
            let (directory, node, controller, client, worktree) = support::empty(remote).await;
            node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
            node.set_connector_fixture("https://service.example.com", &resource.origin);
            configure_label(&directory.path().join("source/package"));
            let package = info(execute(&client, install(worktree, 0)).await)
                .summary
                .reference();
            let original = lifecycle::connect(&client, package.clone()).await;
            let attempt = begin(&client, &original, false).await;
            let LoginState::Browser { url, .. } = state(&client, &attempt, |state| {
                matches!(state, LoginState::Browser { .. })
            })
            .await
            else {
                unreachable!()
            };
            let mutation = if change == "disconnect" {
                Command::DisconnectConnection {
                    connection: original.id,
                    expected_revision: attempt.revision,
                }
            } else {
                Command::UpdateConnection {
                    connection: original.id,
                    expected_revision: attempt.revision,
                    input: Input {
                        fields: if change == "label" {
                            BTreeMap::from([("label".into(), "New label".into())])
                        } else {
                            BTreeMap::new()
                        },
                        secrets: if change == "secret" {
                            BTreeMap::from([(
                                "client_secret".into(),
                                Secret::new("replacement-client-marker".into()),
                            )])
                        } else {
                            BTreeMap::new()
                        },
                        ..Default::default()
                    },
                }
            };
            public(&execute(&client, mutation).await);
            let latest: Value =
                serde_json::from_slice(&lifecycle::stored(&node, original.id).await).unwrap();
            let receipt = execute(
                &client,
                Command::SubmitConnectorInput {
                    attempt: attempt.id,
                    input: callback(&url),
                },
            )
            .await;
            public(&receipt);
            let terminal = state(&client, &attempt, |state| {
                matches!(state, LoginState::Failed(_))
            })
            .await;
            public(&terminal);
            let LoginState::Failed(error) = terminal else {
                unreachable!()
            };
            assert_eq!(error.code, ErrorCode::RevisionConflict);
            let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
                panic!("connections expected")
            };
            public(&rows);
            assert_eq!(rows[0].revision, attempt.revision + 1);
            assert_eq!(
                rows[0].state,
                if change == "label" {
                    State::Connected
                } else {
                    State::NotConnected
                }
            );
            let after: Value =
                serde_json::from_slice(&lifecycle::stored(&node, original.id).await).unwrap();
            assert!(after["login"].is_null());
            for field in [
                "revision",
                "auth_revision",
                "grant_revision",
                "fields",
                "secrets",
                "grant",
            ] {
                assert_eq!(after[field], latest[field], "{remote}/{change}/{field}");
            }
            let (command, scope) = protected(package, worktree, original.id);
            let result = client
                .execute(client.prepare(command).with_plugin(scope))
                .await;
            assert_eq!(
                result.is_ok(),
                change == "label",
                "{remote}/{change}/{result:?}"
            );
            assert_eq!(
                resource.requests.lock().unwrap().len(),
                usize::from(change == "label")
            );
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn discards_pending_edits_after_background_renewal() {
    for remote in [false, true] {
        for outcome in ["denied", "cancelled", "restart"] {
            let restart = outcome == "restart";
            let issuer = server::Server::start(false, false).await;
            let resource = super::super::http::server::Server::start().await;
            let (directory, node, controller, client, worktree) = support::empty(remote).await;
            node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
            node.set_connector_fixture("https://service.example.com", &resource.origin);
            configure_label(&directory.path().join("source/package"));
            let package = info(execute(&client, install(worktree, 0)).await)
                .summary
                .reference();
            let original = lifecycle::connect(&client, package.clone()).await;
            let before: Value =
                serde_json::from_slice(&lifecycle::stored(&node, original.id).await).unwrap();
            let Output::ConnectorLogin(attempt) = execute(
                &client,
                Command::StartConnectorLogin {
                    package: package.clone(),
                    connector: original.connector.clone(),
                    connection: Some(original.id),
                    expected_revision: Some(original.revision),
                    method: original.method,
                    input: Input {
                        redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
                        fields: BTreeMap::from([("label".into(), "Pending label".into())]),
                        ..Default::default()
                    },
                },
            )
            .await
            else {
                panic!("connector attempt expected")
            };
            let LoginState::Browser { url, .. } = state(&client, &attempt, |state| {
                matches!(state, LoginState::Browser { .. })
            })
            .await
            else {
                unreachable!()
            };
            let (command, scope) =
                super::rejections::request(&package, worktree, original.id, true);
            assert!(matches!(
                client.execute(client.prepare(command).with_plugin(scope)).await,
                Ok(Output::PluginHttp(response)) if response.status == 401
            ));
            let renewed: Value =
                serde_json::from_slice(&lifecycle::stored(&node, original.id).await).unwrap();
            assert_eq!(renewed["state"], "connecting");
            assert_eq!(renewed["fields"]["label"], "Pending label");
            let revision = before["grant_revision"].as_u64().unwrap() + 1;
            assert_eq!(renewed["grant_revision"], revision);
            let (node, client) = if restart {
                let interrupted = lifecycle::stored(&node, original.id).await;
                let profile = node.profile().to_owned();
                let node_id = node.id();
                node.shutdown().await.unwrap();
                controller.handle().disconnect(node_id).await;
                drop(client);
                let (database_handle, database) = crate::plugins::database_fixture::open(
                    &profile.join("storage/node.turso"),
                    false,
                )
                .await;
                database
                    .execute(
                        "UPDATE plugin_connections SET body=?1 WHERE id=?2",
                        turso::params![interrupted, original.id.to_string()],
                    )
                    .await
                    .unwrap();
                crate::plugins::database_fixture::close(database_handle, database).await;
                let node = Node::start_empty(&profile).await.unwrap();
                let client = Client::new(if remote {
                    controller.handle().remote(node.link().address())
                } else {
                    node.local()
                });
                (node, client)
            } else if outcome == "cancelled" {
                execute(
                    &client,
                    Command::CancelConnectorLogin {
                        attempt: attempt.id,
                    },
                )
                .await;
                state(&client, &attempt, |state| {
                    matches!(state, LoginState::Cancelled)
                })
                .await;
                (node, client)
            } else {
                let mut denied = reqwest::Url::parse(callback(&url).expose()).unwrap();
                let state_value = denied
                    .query_pairs()
                    .find(|(key, _)| key == "state")
                    .unwrap()
                    .1
                    .into_owned();
                denied.set_query(None);
                denied
                    .query_pairs_mut()
                    .append_pair("state", &state_value)
                    .append_pair("error", "access_denied");
                execute(
                    &client,
                    Command::SubmitConnectorInput {
                        attempt: attempt.id,
                        input: Secret::new(denied.into()),
                    },
                )
                .await;
                state(&client, &attempt, |state| {
                    matches!(state, LoginState::Failed(_))
                })
                .await;
                (node, client)
            };
            let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
                panic!("connections expected")
            };
            public(&rows);
            assert_eq!(rows[0].state, State::Connected, "{remote}/{outcome}");
            let after: Value =
                serde_json::from_slice(&lifecycle::stored(&node, original.id).await).unwrap();
            assert!(after["login"].is_null());
            assert_eq!(after["revision"], original.revision + 1);
            assert_eq!(after["grant_revision"], revision);
            assert_eq!(after["grant"], renewed["grant"]);
            for field in ["fields", "secrets", "identity", "auth_revision"] {
                assert_eq!(after[field], before[field], "{remote}/{outcome}/{field}");
            }
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}
