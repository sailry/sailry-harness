use super::*;
use sailry_protocol::plugin::{Context, connector::State, http};
use std::sync::atomic::Ordering;

pub(super) fn request(
    package: &plugin::Reference,
    worktree: WorktreeId,
    id: plugin::connector::Id,
    rejected: bool,
) -> (Command, Context) {
    (
        Command::RequestPluginHttp(http::Request {
            url: format!(
                "https://service.example.com/{}",
                if rejected { "unauthorized" } else { "ok" }
            ),
            method: if rejected { "POST" } else { "GET" }.into(),
            headers: BTreeMap::new(),
            body: rejected.then(|| "write-once".into()),
            credential: None,
            connector: Some("account".into()),
            connection: Some(id),
            idempotent: false,
            timeout_ms: 2000,
        }),
        Context {
            package: package.clone(),
            worktree: Some(worktree),
            session: None,
            turn: None,
            invocation: None,
            surface: Default::default(),
        },
    )
}

#[tokio::test]
async fn expires_accounts_without_refresh_support() {
    for remote in [false, true] {
        for missing in [false, true] {
            for rejected in [false, true] {
                let issuer = server::Server::respond(false, true, !missing, None).await;
                let resource = super::super::http::server::Server::start().await;
                let (directory, node, controller, client, worktree) = support::empty(remote).await;
                node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
                node.set_connector_fixture("https://service.example.com", &resource.origin);
                let root = directory.path().join("source/package");
                configure(&root);
                if !missing {
                    let filename = root.join("plugin.json");
                    let mut manifest: serde_json::Value =
                        serde_json::from_slice(&fs::read(&filename).unwrap()).unwrap();
                    manifest["extensions"]["dev.sailry.platform"]["connectors"][0]["auth"][0]["refresh"] =
                        json!(false);
                    fs::write(filename, manifest.to_string()).unwrap();
                }
                let package = info(execute(&client, install(worktree, 0)).await)
                    .summary
                    .reference();
                let row = lifecycle::connect(&client, package.clone()).await;
                let before: serde_json::Value =
                    serde_json::from_slice(&lifecycle::stored(&node, row.id)).unwrap();
                assert_eq!(before["grant"]["refresh"].is_null(), missing);
                if !rejected {
                    tokio::time::sleep(Duration::from_millis(1100)).await;
                }
                let (command, scope) = request(&package, worktree, row.id, rejected);
                let call = client.prepare(command).with_plugin(scope);
                let error = client.execute(call.clone()).await.unwrap_err();
                assert_eq!(
                    error.code,
                    ErrorCode::Expired,
                    "{remote}/{missing}/{rejected}"
                );
                assert_eq!(client.execute(call).await.unwrap_err(), error);
                assert_eq!(issuer.bodies.lock().unwrap().len(), 1);
                {
                    let calls = resource.requests.lock().unwrap();
                    assert_eq!(calls.len(), usize::from(rejected));
                    if rejected {
                        assert!(calls[0].0.starts_with("POST /unauthorized "));
                        assert_eq!(calls[0].1, "write-once");
                    }
                }
                let after: serde_json::Value =
                    serde_json::from_slice(&lifecycle::stored(&node, row.id)).unwrap();
                assert!(after["grant"].is_null());
                for field in ["fields", "secrets", "auth_revision", "revision"] {
                    assert_eq!(
                        after[field], before[field],
                        "{remote}/{missing}/{rejected}/{field}"
                    );
                }
                let Output::Connections(rows) = execute(&client, Command::ListConnections).await
                else {
                    panic!("connections expected")
                };
                assert_eq!(rows[0].state, State::Expired);
                controller.close().await.unwrap();
                node.shutdown().await.unwrap();
            }
        }
    }
}

async fn attempt(
    client: &Client,
    package: plugin::Reference,
    previous: Option<&plugin::connector::Connection>,
) -> Attempt {
    let Output::ConnectorLogin(attempt) = execute(
        client,
        Command::StartConnectorLogin {
            package,
            connector: "account".into(),
            connection: previous.map(|row| row.id),
            expected_revision: previous.map(|row| row.revision),
            method: Kind::Oauth2,
            input: Input {
                redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
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
        panic!("connector login expected")
    };
    let LoginState::Browser { url, .. } = state(client, &attempt, |state| {
        matches!(state, LoginState::Browser { .. })
    })
    .await
    else {
        unreachable!()
    };
    execute(
        client,
        Command::SubmitConnectorInput {
            attempt: attempt.id,
            input: callback(&url),
        },
    )
    .await;
    attempt
}

#[tokio::test]
async fn preserves_accounts_when_new_authorization_fails() {
    for remote in [false, true] {
        for initial in [false, true] {
            for (status, rejected) in [
                (400, "invalid_client"),
                (400, "unauthorized_client"),
                (503, "invalid_client"),
                (1, "invalid_client"),
            ] {
                let issuer = server::Server::respond(false, false, true, Some(rejected)).await;
                let (directory, node, controller, client, worktree) = support::empty(remote).await;
                node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
                configure(&directory.path().join("source/package"));
                let package = info(execute(&client, install(worktree, 0)).await)
                    .summary
                    .reference();
                let previous = if initial {
                    None
                } else {
                    Some(lifecycle::connect(&client, package.clone()).await)
                };
                let before = previous.as_ref().map(|row| {
                    serde_json::from_slice::<serde_json::Value>(&lifecycle::stored(&node, row.id))
                        .unwrap()
                });
                issuer.status.store(status, Ordering::SeqCst);
                let attempt = attempt(&client, package, previous.as_ref()).await;
                let LoginState::Failed(error) = state(&client, &attempt, |state| {
                    matches!(state, LoginState::Failed(_))
                })
                .await
                else {
                    unreachable!()
                };
                assert_eq!(
                    error.code == ErrorCode::PermissionDenied,
                    status == 400,
                    "{remote}/{initial}/{status}"
                );
                let Output::Connections(rows) = execute(&client, Command::ListConnections).await
                else {
                    panic!("connections expected")
                };
                assert_eq!(
                    rows[0].state,
                    if initial {
                        State::Failed
                    } else {
                        State::Connected
                    }
                );
                let after: serde_json::Value =
                    serde_json::from_slice(&lifecycle::stored(&node, rows[0].id)).unwrap();
                if initial {
                    assert!(after["grant"].is_null());
                } else {
                    let before = before.unwrap();
                    assert_eq!(after["grant"], before["grant"]);
                    assert_eq!(after["secrets"], before["secrets"]);
                    assert_eq!(after["grant_revision"], before["grant_revision"]);
                    assert_eq!(after["auth_revision"], before["auth_revision"]);
                    assert_eq!(after["checked_at_ms"], before["checked_at_ms"]);
                    assert_eq!(after["revision"], before["revision"].as_u64().unwrap() + 1);
                }
                assert!(after["login"].is_null());
                controller.close().await.unwrap();
                node.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn expires_rejected_rotations() {
    for remote in [false, true] {
        let issuer = server::Server::start(false, false).await;
        let resource = super::super::http::server::Server::start().await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
        node.set_connector_fixture("https://service.example.com", &resource.origin);
        configure(&directory.path().join("source/package"));
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let row = lifecycle::connect(&client, package.clone()).await;
        let before: serde_json::Value =
            serde_json::from_slice(&lifecycle::stored(&node, row.id)).unwrap();
        let revision = before["grant_revision"].as_u64().unwrap();
        for (index, state) in ["connected", "expired"].into_iter().enumerate() {
            let (command, scope) = request(&package, worktree, row.id, true);
            let result = client
                .execute(client.prepare(command).with_plugin(scope))
                .await;
            assert!(
                matches!(result, Ok(Output::PluginHttp(ref response)) if response.status == 401),
                "{remote}/{index}"
            );
            let after: serde_json::Value =
                serde_json::from_slice(&lifecycle::stored(&node, row.id)).unwrap();
            assert_eq!(after["state"], state, "{remote}/{index}");
            assert_eq!(after["grant"].is_null(), index == 1);
            assert_eq!(
                after["grant_revision"].as_u64().unwrap(),
                revision + 1 + index as u64
            );
            assert_eq!(issuer.bodies.lock().unwrap().len(), 2, "one refresh only");
        }
        assert_eq!(resource.requests.lock().unwrap().len(), 2);
        let (command, scope) = request(&package, worktree, row.id, false);
        assert_eq!(
            client
                .execute(client.prepare(command).with_plugin(scope))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn invalidates_rejected_refreshes_and_health_checks() {
    for remote in [false, true] {
        for probe in [false, true] {
            let issuer = server::Server::respond(false, false, true, Some("invalid_client")).await;
            let resource =
                super::super::http::server::Server::respond(Some("{}".into()), None).await;
            let (directory, node, controller, client, worktree) = support::empty(remote).await;
            node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
            node.set_connector_fixture("https://service.example.com", &resource.origin);
            let root = directory.path().join("source/package");
            configure(&root);
            let filename = root.join("plugin.json");
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(&filename).unwrap()).unwrap();
            manifest["extensions"]["dev.sailry.platform"]["connectors"][0]["health"] =
                json!({"request":"https://service.example.com/health","ok":[200]});
            fs::write(filename, manifest.to_string()).unwrap();
            let package = info(execute(&client, install(worktree, 0)).await)
                .summary
                .reference();
            let row = lifecycle::connect(&client, package.clone()).await;
            let before: serde_json::Value =
                serde_json::from_slice(&lifecycle::stored(&node, row.id)).unwrap();
            let original = if probe {
                resource.status.store(401, Ordering::SeqCst);
                client.prepare(Command::ProbeConnection { connection: row.id })
            } else {
                issuer.status.store(400, Ordering::SeqCst);
                let (command, scope) = request(&package, worktree, row.id, true);
                client.prepare(command).with_plugin(scope)
            };
            let count = resource.requests.lock().unwrap().len();
            let expected = count + 1 + usize::from(probe);
            assert_eq!(issuer.bodies.lock().unwrap().len(), 1);
            assert_eq!(
                client.execute(original).await.unwrap_err().code,
                if probe {
                    ErrorCode::Expired
                } else {
                    ErrorCode::PermissionDenied
                }
            );
            {
                let calls = resource.requests.lock().unwrap();
                assert_eq!(calls.len(), expected, "{remote}/{probe}");
                for (headers, body) in &calls[count..] {
                    if probe {
                        assert!(headers.starts_with("GET /health "));
                        assert!(body.is_empty());
                    } else {
                        assert!(headers.starts_with("POST /unauthorized "));
                        assert_eq!(body, "write-once");
                    }
                }
            }
            assert_eq!(issuer.bodies.lock().unwrap().len(), 2, "{remote}/{probe}");
            let after: serde_json::Value =
                serde_json::from_slice(&lifecycle::stored(&node, row.id)).unwrap();
            assert_eq!(after["state"], "expired");
            assert!(after["grant"].is_null());
            assert_eq!(
                after["grant_revision"].as_u64().unwrap(),
                before["grant_revision"].as_u64().unwrap() + 1 + u64::from(probe)
            );
            for field in ["fields", "secrets", "auth_revision", "revision"] {
                assert_eq!(after[field], before[field], "{remote}/{probe}/{field}");
            }
            let (command, scope) = request(&package, worktree, row.id, false);
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(scope))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
            assert_eq!(resource.requests.lock().unwrap().len(), expected);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}
