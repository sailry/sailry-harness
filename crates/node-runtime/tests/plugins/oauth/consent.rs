use super::*;
use sailry_protocol::plugin::{Context, connector::State, http};

#[tokio::test]
async fn confirms_changed_permissions_before_reauthorization() {
    for remote in [false, true] {
        let issuer = server::Server::start(false, false).await;
        let resource = super::super::http::server::Server::start().await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
        node.set_connector_fixture("https://service.example.com", &resource.origin);
        let root = directory.path().join("source/package");
        configure(&root);
        let package = info(execute(&client, install(worktree, 0)).await);
        let row = super::lifecycle::connect(&client, package.summary.reference()).await;
        let filename = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&filename).unwrap()).unwrap();
        manifest["version"] = json!("1.0.1");
        manifest["extensions"]["dev.sailry.platform"]["connectors"][0]["auth"][0]["scopes"] =
            json!(["read", "additional.read"]);
        fs::write(filename, manifest.to_string()).unwrap();
        let package = info(execute(&client, install(worktree, package.summary.revision)).await);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let scope = Context {
            package: package.summary.reference(),
            worktree: Some(worktree),
            session: None,
            turn: None,
            invocation: None,
            surface: Default::default(),
        };
        let request = || {
            client
                .prepare(Command::RequestPluginHttp(http::Request {
                    url: "https://service.example.com/ok".into(),
                    method: "GET".into(),
                    headers: BTreeMap::new(),
                    body: None,
                    credential: None,
                    connector: Some("account".into()),
                    connection: Some(row.id),
                    idempotent: false,
                    timeout_ms: 2000,
                }))
                .with_plugin(scope.clone())
        };
        let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
            panic!("connections expected")
        };
        assert_eq!(rows[0].state, State::NeedsConfirmation);
        assert!(rows[0].can_disconnect);
        assert_eq!(
            client.execute(request()).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        assert!(resource.requests.lock().unwrap().is_empty());
        let Output::Connection(declined) = execute(
            &client,
            Command::ConfirmConnectorConsent {
                connection: row.id,
                expected_revision: row.revision,
                accept: false,
            },
        )
        .await
        else {
            panic!("connection expected")
        };
        assert_eq!(declined.revision, row.revision);
        assert_eq!(declined.state, State::NeedsConfirmation);
        let confirm = client.prepare(Command::ConfirmConnectorConsent {
            connection: row.id,
            expected_revision: row.revision,
            accept: true,
        });
        let output = client.execute(confirm.clone()).await.unwrap();
        assert_eq!(client.execute(confirm).await.unwrap(), output);
        let Output::Connection(accepted) = output else {
            panic!("connection expected")
        };
        assert_eq!(accepted.revision, row.revision + 1);
        assert_eq!(accepted.state, State::NotConnected);
        assert!(!accepted.can_disconnect);
        let Output::ConnectorLogin(attempt) = execute(
            &client,
            Command::StartConnectorLogin {
                package: package.summary.reference(),
                connector: "account".into(),
                connection: Some(row.id),
                expected_revision: Some(accepted.revision),
                method: Kind::Oauth2,
                input: Input {
                    redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
                    ..Default::default()
                },
            },
        )
        .await
        else {
            panic!("login expected")
        };
        let LoginState::Browser { url, .. } = state(&client, &attempt, |state| {
            matches!(state, LoginState::Browser { .. })
        })
        .await
        else {
            unreachable!()
        };
        assert!(url.contains("additional.read"));
        execute(
            &client,
            Command::SubmitConnectorInput {
                attempt: attempt.id,
                input: callback(&url),
            },
        )
        .await;
        let LoginState::Connected(connected) = state(&client, &attempt, |state| {
            matches!(state, LoginState::Connected(_))
        })
        .await
        else {
            unreachable!()
        };
        assert_eq!(connected.id, row.id);
        assert!(
            matches!(client.execute(request()).await.unwrap(),Output::PluginHttp(response) if response.status==201)
        );
        assert_eq!(resource.requests.lock().unwrap().len(), 1);
        let public = serde_json::to_string(&(rows, accepted, connected)).unwrap();
        for marker in [
            "private-access-marker",
            "private-refresh-marker",
            "private-client-marker",
        ] {
            assert!(!public.contains(marker));
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn projects_saved_credentials_independently_from_status() {
    for remote in [false, true] {
        let issuer = server::Server::start(false, false).await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
        configure(&directory.path().join("source/package"));
        let package = info(execute(&client, install(worktree, 0)).await);
        let row = lifecycle::connect(&client, package.summary.reference()).await;
        let original: serde_json::Value =
            serde_json::from_slice(&lifecycle::stored(&node, row.id)).unwrap();
        let profile = node.profile().to_owned();
        let node_id = node.id();
        node.shutdown().await.unwrap();
        controller.handle().disconnect(node_id).await;
        drop(client);
        for status in [State::Failed, State::NeedsConfirmation] {
            for credentials in [false, true] {
                // Isolated saved records exercise equal public states with different private accounts.
                let mut record = original.clone();
                record["state"] = serde_json::to_value(status).unwrap();
                if !credentials {
                    record["grant"] = serde_json::Value::Null;
                    record["secrets"] = json!({});
                }
                let database =
                    rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
                database
                    .execute(
                        "UPDATE plugin_connections SET body=?1,revision=?2 WHERE id=?3",
                        rusqlite::params![
                            serde_json::to_vec(&record).unwrap(),
                            row.revision as i64,
                            row.id.to_string()
                        ],
                    )
                    .unwrap();
                database.close().unwrap();
                let node = Node::start_empty(&profile).await.unwrap();
                let client = Client::new(if remote {
                    controller.handle().remote(node.link().address())
                } else {
                    node.local()
                });
                let Output::Connections(rows) = execute(&client, Command::ListConnections).await
                else {
                    panic!("connections expected")
                };
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].state, status);
                assert_eq!(
                    rows[0].can_disconnect, credentials,
                    "{remote}/{status:?}/{credentials}"
                );
                let mut feed = client.subscribe_connections().await.unwrap();
                let Update::ConnectionsChanged(snapshot) = feed.next().await.unwrap() else {
                    panic!("connections snapshot expected")
                };
                assert_eq!(snapshot.connections, rows);
                let public = serde_json::to_string(&snapshot).unwrap();
                for secret in [
                    "private-access-marker",
                    "private-refresh-marker",
                    "private-client-marker",
                ] {
                    assert!(!public.contains(secret));
                }
                if credentials {
                    let Output::Connection(disconnected) = execute(
                        &client,
                        Command::DisconnectConnection {
                            connection: row.id,
                            expected_revision: rows[0].revision,
                        },
                    )
                    .await
                    else {
                        panic!("connection expected")
                    };
                    assert_eq!(disconnected.fields, rows[0].fields);
                    assert_eq!(disconnected.state, State::NotConnected);
                    assert!(!disconnected.can_disconnect);
                }
                node.shutdown().await.unwrap();
                controller.handle().disconnect(node_id).await;
            }
        }
        controller.close().await.unwrap();
    }
}
