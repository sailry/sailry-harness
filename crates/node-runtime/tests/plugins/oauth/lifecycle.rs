use super::*;
use sailry_protocol::plugin::{Context, connector::State, http};
use std::sync::atomic::Ordering;

pub(super) async fn connect(
    client: &Client,
    package: plugin::Reference,
) -> plugin::connector::Connection {
    let Output::ConnectorLogin(attempt) = client
        .execute(client.prepare(Command::StartConnectorLogin {
            package,
            connector: "account".into(),
            connection: None,
            expected_revision: None,
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
        }))
        .await
        .unwrap()
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
    let LoginState::Connected(row) = state(client, &attempt, |state| {
        matches!(state, LoginState::Connected(_))
    })
    .await
    else {
        unreachable!()
    };
    row
}

pub(super) async fn stored(node: &Node, id: plugin::connector::Id) -> Vec<u8> {
    node.with_storage(move |db| {
        Box::pin(async move {
            db.prepare("SELECT body FROM plugin_connections WHERE id=?1")
                .await
                .map_err(crate::plugins::database_fixture::fault)?
                .query_row([id.to_string()])
                .await
                .map_err(crate::plugins::database_fixture::fault)?
                .get(0)
                .map_err(crate::plugins::database_fixture::fault)
        })
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn rotates_without_losing_refresh_credentials() {
    for remote in [false, true] {
        let issuer = server::Server::start(false, true).await;
        let resource = super::super::http::server::Server::start().await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture("https://issuer.example.com", &issuer.endpoint);
        node.set_connector_fixture("https://service.example.com", &resource.origin);
        configure(&directory.path().join("source/package"));
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let original = connect(&client, package.clone()).await;
        {
            let bodies = issuer.bodies.lock().unwrap();
            let fields: BTreeMap<_, _> =
                url::form_urlencoded::parse(bodies[0].as_bytes()).collect();
            assert_eq!(fields["grant_type"], "authorization_code");
            assert_eq!(fields["code"], "single-code");
            assert_eq!(
                fields["redirect_uri"],
                "http://127.0.0.1:43191/auth/callback"
            );
            assert!(fields["code_verifier"].len() >= 43);
            assert_eq!(fields["client_id"], "registered-client");
            assert_eq!(fields["client_secret"], "private-client-marker");
        }
        let before: serde_json::Value =
            serde_json::from_slice(&stored(&node, original.id).await).unwrap();
        let Output::Connection(row) = execute(
            &client,
            Command::UpdateConnection {
                connection: original.id,
                expected_revision: original.revision,
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
            panic!("connection expected")
        };
        assert_eq!(row.revision, original.revision + 1);
        assert_eq!(row.state, State::Connected);
        let after: serde_json::Value =
            serde_json::from_slice(&stored(&node, row.id).await).unwrap();
        for field in ["auth_revision", "grant", "grant_revision", "secrets"] {
            assert_eq!(after[field], before[field]);
        }
        let other = Client::new(if remote {
            node.local()
        } else {
            controller.handle().remote(node.link().address())
        });
        let scope = Context {
            package,
            worktree: Some(worktree),
            session: None,
            turn: None,
            invocation: None,
            surface: Default::default(),
        };
        let request = |path: &str, method: &str| {
            client
                .prepare(Command::RequestPluginHttp(http::Request {
                    url: format!("https://service.example.com{path}"),
                    method: method.into(),
                    headers: BTreeMap::new(),
                    body: (method == "POST").then(|| "write-once".into()),
                    credential: None,
                    connector: Some("account".into()),
                    connection: Some(row.id),
                    idempotent: false,
                    timeout_ms: 2000,
                }))
                .with_plugin(scope.clone())
        };
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let (first, second) = tokio::join!(
            client.execute(request("/ok", "GET")),
            other.execute(request("/other", "GET"))
        );
        assert!(first.is_ok(), "{first:?}");
        assert!(second.is_ok(), "{second:?}");
        assert_eq!(issuer.bodies.lock().unwrap().len(), 2);
        {
            let bodies = issuer.bodies.lock().unwrap();
            let fields: BTreeMap<_, _> =
                url::form_urlencoded::parse(bodies[1].as_bytes()).collect();
            assert_eq!(fields["grant_type"], "refresh_token");
            assert_eq!(fields["refresh_token"], "private-refresh-marker");
            assert_eq!(fields["client_id"], "registered-client");
            assert_eq!(fields["client_secret"], "private-client-marker");
            assert!(!fields.contains_key("code"));
            assert!(!fields.contains_key("code_verifier"));
        }

        let original = request("/unauthorized", "POST");
        let before = resource.requests.lock().unwrap().len();
        let output = client.execute(original.clone()).await.unwrap();
        assert!(matches!(&output, Output::PluginHttp(response) if response.status == 401));
        assert_eq!(client.execute(original).await.unwrap(), output);
        assert_eq!(resource.requests.lock().unwrap().len() - before, 1);
        assert_eq!(issuer.bodies.lock().unwrap().len(), 3);
        // An accepted request confirms the rotated grant; a 401 before that expires it.
        assert!(client.execute(request("/ok", "GET")).await.is_ok());

        for status in [503, 403, 1] {
            issuer.status.store(status, Ordering::SeqCst);
            let before = stored(&node, row.id).await;
            let original = request("/unauthorized", "POST");
            let count = resource.requests.lock().unwrap().len();
            let error = client.execute(original.clone()).await.unwrap_err();
            assert_ne!(error.code, ErrorCode::PermissionDenied, "{status}");
            assert_eq!(client.execute(original).await.unwrap_err(), error);
            assert_eq!(stored(&node, row.id).await, before, "{status}");
            assert_eq!(resource.requests.lock().unwrap().len() - count, 1);
        }

        issuer.status.store(200, Ordering::SeqCst);
        assert!(client.execute(request("/ok", "GET")).await.is_ok());
        issuer.status.store(400, Ordering::SeqCst);
        assert_eq!(
            client
                .execute(request("/unauthorized", "POST"))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
            panic!("connections expected")
        };
        assert_eq!(rows[0].state, State::Expired);
        assert!(rows[0].configured.contains(&"client_secret".into()));
        let rejected: serde_json::Value =
            serde_json::from_slice(&stored(&node, row.id).await).unwrap();
        assert!(rejected["grant"].is_null());
        drop(other);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
