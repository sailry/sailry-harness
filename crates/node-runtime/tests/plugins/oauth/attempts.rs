//! New authorization failures do not invalidate an existing account.
use super::*;
use sailry_protocol::plugin::connector::{Connection, State};
use serde_json::Value;
use std::sync::atomic::Ordering;

pub(super) async fn start(client: &Client, row: &Connection, redirect: bool) -> Attempt {
    let Output::ConnectorLogin(attempt) = execute(
        client,
        Command::StartConnectorLogin {
            package: row.package.clone(),
            connector: row.connector.clone(),
            connection: Some(row.id),
            expected_revision: Some(row.revision),
            method: row.method,
            input: Input {
                redirect: redirect.then(|| "http://127.0.0.1:43191/auth/callback".into()),
                ..Default::default()
            },
        },
    )
    .await
    else {
        panic!("connector attempt expected")
    };
    attempt
}

fn restored(node: &Node, original: &Connection, before: &Value) {
    let after: Value = serde_json::from_slice(&lifecycle::stored(node, original.id)).unwrap();
    assert_eq!(after["state"], "connected");
    assert!(after["login"].is_null());
    assert_eq!(after["revision"], original.revision + 1);
    for field in [
        "grant",
        "secrets",
        "fields",
        "identity",
        "auth_revision",
        "grant_revision",
        "checked_at_ms",
    ] {
        assert_eq!(after[field], before[field], "{field}");
    }
}

#[tokio::test]
async fn restores_denied_and_timed_out_accounts() {
    for remote in [false, true] {
        for timeout in [false, true] {
            let issuer = server::Server::start(false, false).await;
            let resource = super::super::http::server::Server::start().await;
            let (directory, node, controller, client, worktree) = support::empty(remote).await;
            node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
            node.set_connector_fixture("https://service.example.com", &resource.origin);
            configure(&directory.path().join("source/package"));
            let package = info(execute(&client, install(worktree, 0)).await)
                .summary
                .reference();
            let original = lifecycle::connect(&client, package).await;
            let before = serde_json::from_slice(&lifecycle::stored(&node, original.id)).unwrap();
            let attempt = start(&client, &original, true).await;
            let LoginState::Browser { url, .. } = state(&client, &attempt, |state| {
                matches!(state, LoginState::Browser { .. })
            })
            .await
            else {
                unreachable!()
            };
            let input = if timeout {
                issuer.status.store(0, Ordering::SeqCst);
                callback(&url)
            } else {
                let mut redirect = reqwest::Url::parse(callback(&url).expose()).unwrap();
                let state = redirect
                    .query_pairs()
                    .find(|(key, _)| key == "state")
                    .unwrap()
                    .1
                    .into_owned();
                redirect.set_query(None);
                redirect
                    .query_pairs_mut()
                    .append_pair("state", &state)
                    .append_pair("error", "access_denied");
                Secret::new(redirect.into())
            };
            execute(
                &client,
                Command::SubmitConnectorInput {
                    attempt: attempt.id,
                    input,
                },
            )
            .await;
            let failed = tokio::time::timeout(Duration::from_secs(40), async {
                let mut updates = client.subscribe_connector_login(attempt.id).await.unwrap();
                loop {
                    let Update::ConnectorLogin(update) = updates.next().await.unwrap() else {
                        panic!("login update expected")
                    };
                    if let LoginState::Failed(error) = update.state {
                        break error;
                    }
                    assert!(update.state.active());
                }
            })
            .await
            .unwrap();
            assert_eq!(
                failed.code,
                if timeout {
                    ErrorCode::Unavailable
                } else {
                    ErrorCode::PermissionDenied
                }
            );
            restored(&node, &original, &before);
            let (command, scope) =
                super::rejections::request(&original.package, worktree, original.id, false);
            assert!(
                client
                    .execute(client.prepare(command).with_plugin(scope))
                    .await
                    .is_ok()
            );
            assert_eq!(resource.requests.lock().unwrap().len(), 1);
            assert_eq!(
                issuer.bodies.lock().unwrap().len(),
                1 + usize::from(timeout)
            );
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn retries_poll_failures_and_restores_expiry() {
    for remote in [false, true] {
        // Status 1 closes the connection without a response.
        for (status, error) in [
            (503, "authorization_pending"),
            (1, "authorization_pending"),
            (400, "authorization_pending"),
            (400, "expired_token"),
        ] {
            let issuer = server::Server::serve(false, false, true, Some(error), true).await;
            let (directory, node, controller, client, worktree) = support::empty(remote).await;
            node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
            let root = directory.path().join("source/package");
            configure(&root);
            let filename = root.join("plugin.json");
            let mut manifest: Value =
                serde_json::from_slice(&fs::read(&filename).unwrap()).unwrap();
            let auth =
                &mut manifest["extensions"]["dev.sailry.platform"]["connectors"][0]["auth"][0];
            auth["flow"] = json!("device_code");
            auth["device"] = json!("https://issuer.example.com/device");
            fs::write(filename, manifest.to_string()).unwrap();
            let package = info(execute(&client, install(worktree, 0)).await)
                .summary
                .reference();
            let Output::ConnectorLogin(initial) = execute(
                &client,
                Command::StartConnectorLogin {
                    package,
                    connector: "account".into(),
                    connection: None,
                    expected_revision: None,
                    method: Kind::Oauth2,
                    input: Input {
                        fields: BTreeMap::from([("client_id".into(), "registered-client".into())]),
                        secrets: BTreeMap::from([(
                            "client_secret".into(),
                            Secret::new("private-client-marker".into()),
                        )]),
                        ..Default::default()
                    },
                },
            )
            .await
            else {
                panic!("connector attempt expected")
            };
            let LoginState::Connected(original) = state(&client, &initial, |state| {
                matches!(state, LoginState::Connected(_))
            })
            .await
            else {
                unreachable!()
            };
            let before = serde_json::from_slice(&lifecycle::stored(&node, original.id)).unwrap();
            issuer.status.store(status, Ordering::SeqCst);
            let attempt = start(&client, &original, false).await;
            state(&client, &attempt, |state| {
                matches!(state, LoginState::Device { .. })
            })
            .await;
            let result = state(&client, &attempt, |state| !state.active()).await;
            if status == 400 {
                let code = if error == "expired_token" {
                    ErrorCode::PermissionDenied
                } else {
                    ErrorCode::Expired
                };
                assert!(
                    matches!(result, LoginState::Failed(ref error) if error.code == code),
                    "{remote}/{error}: {result:?}"
                );
                restored(&node, &original, &before);
            } else {
                assert!(
                    matches!(result, LoginState::Connected(ref row) if row.state == State::Connected),
                    "{remote}/{status}: {result:?}"
                );
                assert_eq!(issuer.bodies.lock().unwrap().len(), 5);
            }
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn rejects_probes_during_sign_in() {
    for remote in [false, true] {
        let issuer = server::Server::start(false, false).await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
        configure(&directory.path().join("source/package"));
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let original = lifecycle::connect(&client, package).await;
        let before = serde_json::from_slice(&lifecycle::stored(&node, original.id)).unwrap();
        let attempt = start(&client, &original, true).await;
        state(&client, &attempt, |state| {
            matches!(state, LoginState::Browser { .. })
        })
        .await;
        let probe = client.prepare(Command::ProbeConnection {
            connection: original.id,
        });
        assert_eq!(
            client.execute(probe).await.unwrap_err().code,
            ErrorCode::Busy
        );
        let pending: Value =
            serde_json::from_slice(&lifecycle::stored(&node, original.id)).unwrap();
        assert_eq!(pending["state"], "connecting");
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
        restored(&node, &original, &before);
        let probe = client.prepare(Command::ProbeConnection {
            connection: original.id,
        });
        assert!(matches!(
            client.execute(probe).await.unwrap(),
            Output::Connection(row) if row.state == State::Connected
        ));
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
