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
