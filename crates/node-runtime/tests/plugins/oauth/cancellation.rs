use super::*;
use plugin::{Context, connector::State, http};

#[derive(Clone, Copy, Debug)]
enum Change {
    Unchanged,
    PendingSecret,
    Disabled,
    Disconnected,
    Removed,
    ReplacedSecret,
}

#[tokio::test]
async fn preserves_healthy_accounts_and_concurrent_changes() {
    for remote in [false, true] {
        for change in [
            Change::Unchanged,
            Change::PendingSecret,
            Change::Disabled,
            Change::Disconnected,
            Change::Removed,
            Change::ReplacedSecret,
        ] {
            let issuer = server::Server::start(false, false).await;
            let resource = super::super::http::server::Server::start().await;
            let (directory, node, controller, client, worktree) = support::empty(remote).await;
            node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
            node.set_connector_fixture("https://service.example.com", &resource.origin);
            configure(&directory.path().join("source/package"));
            let package = info(execute(&client, install(worktree, 0)).await)
                .summary
                .reference();
            let original = lifecycle::connect(&client, package.clone()).await;
            let before: serde_json::Value =
                serde_json::from_slice(&lifecycle::stored(&node, original.id)).unwrap();
            let mut input = Input {
                redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
                ..Default::default()
            };
            if matches!(change, Change::PendingSecret) {
                input.secrets.insert(
                    "client_secret".into(),
                    Secret::new("pending-client-marker".into()),
                );
            }
            let Output::ConnectorLogin(attempt) = execute(
                &client,
                Command::StartConnectorLogin {
                    package: package.clone(),
                    connector: original.connector.clone(),
                    connection: Some(original.id),
                    expected_revision: Some(original.revision),
                    method: Kind::Oauth2,
                    input,
                },
            )
            .await
            else {
                panic!("connector attempt expected")
            };
            state(&client, &attempt, |state| {
                matches!(state, LoginState::Browser { .. })
            })
            .await;
            let started: serde_json::Value =
                serde_json::from_slice(&lifecycle::stored(&node, original.id)).unwrap();
            let mutation = match change {
                Change::Unchanged | Change::PendingSecret => None,
                Change::Disabled => Some(Command::SetConnectionEnabled {
                    connection: original.id,
                    expected_revision: attempt.revision,
                    enabled: false,
                }),
                Change::Disconnected => Some(Command::DisconnectConnection {
                    connection: original.id,
                    expected_revision: attempt.revision,
                }),
                Change::Removed => Some(Command::RemoveConnection {
                    connection: original.id,
                    expected_revision: attempt.revision,
                }),
                Change::ReplacedSecret => Some(Command::UpdateConnection {
                    connection: original.id,
                    expected_revision: attempt.revision,
                    input: Input {
                        secrets: BTreeMap::from([(
                            "client_secret".into(),
                            Secret::new("replacement-client-marker".into()),
                        )]),
                        ..Default::default()
                    },
                }),
            };
            if let Some(mutation) = mutation {
                execute(&client, mutation).await;
            }
            let retained =
                (!matches!(change, Change::Removed)).then(|| lifecycle::stored(&node, original.id));
            let cancel = client.prepare(Command::CancelConnectorLogin {
                attempt: attempt.id,
            });
            let receipt = client.execute(cancel.clone()).await.unwrap();
            assert_eq!(client.execute(cancel).await.unwrap(), receipt);
            assert_eq!(
                state(&client, &attempt, |state| matches!(
                    state,
                    LoginState::Cancelled
                ))
                .await,
                LoginState::Cancelled
            );
            let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
                panic!("connections expected")
            };
            let healthy = matches!(change, Change::Unchanged | Change::PendingSecret);
            if healthy {
                assert_eq!(rows[0].state, State::Connected);
                assert_eq!(rows[0].revision, attempt.revision);
                let after: serde_json::Value =
                    serde_json::from_slice(&lifecycle::stored(&node, original.id)).unwrap();
                for field in [
                    "method",
                    "fields",
                    "secrets",
                    "grant",
                    "identity",
                    "bound",
                    "state",
                    "checked_at_ms",
                ] {
                    assert_eq!(after[field], before[field], "{remote}/{change:?}/{field}");
                }
                assert_eq!(after["grant_revision"], started["grant_revision"]);
                assert_eq!(
                    after["grant_revision"].as_u64().unwrap(),
                    before["grant_revision"].as_u64().unwrap()
                        + u64::from(matches!(change, Change::PendingSecret))
                );
                assert_eq!(after["auth_revision"], started["auth_revision"]);
                if matches!(change, Change::PendingSecret) {
                    assert_ne!(after["auth_revision"], before["auth_revision"]);
                }
            } else if matches!(change, Change::Removed) {
                assert!(rows.is_empty());
            } else {
                assert_eq!(
                    lifecycle::stored(&node, original.id),
                    retained.unwrap(),
                    "{remote}/{change:?}"
                );
            }
            let request = client
                .prepare(Command::RequestPluginHttp(http::Request {
                    url: "https://service.example.com/ok".into(),
                    method: "GET".into(),
                    headers: BTreeMap::new(),
                    body: None,
                    credential: None,
                    connector: Some(original.connector.clone()),
                    connection: Some(original.id),
                    idempotent: false,
                    timeout_ms: 2000,
                }))
                .with_plugin(Context {
                    package,
                    worktree: Some(worktree),
                    session: None,
                    turn: None,
                    invocation: None,
                    surface: Default::default(),
                });
            let result = client.execute(request).await;
            if healthy {
                assert!(result.is_ok(), "{remote}/{change:?}/{result:?}");
                let requests = resource.requests.lock().unwrap();
                assert_eq!(requests.len(), 1);
                assert!(
                    requests[0]
                        .0
                        .contains("authorization: Bearer private-access-marker\r\n")
                );
            } else {
                assert!(result.is_err(), "{remote}/{change:?}");
                assert!(resource.requests.lock().unwrap().is_empty());
            }
            assert_eq!(issuer.bodies.lock().unwrap().len(), 1);
            let public = serde_json::to_string(&(rows, receipt)).unwrap();
            for secret in [
                "private-access-marker",
                "private-client-marker",
                "pending-client-marker",
                "replacement-client-marker",
            ] {
                assert!(!public.contains(secret));
            }
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}
