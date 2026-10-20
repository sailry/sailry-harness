//! Connection service acceptance uses controller commands on both Link paths.
use super::super::http::server;
use super::super::*;
use plugin::{
    Context,
    connector::{self, Input, Kind, LoginState, State},
    http,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const ORIGIN: &str = "https://service.example.com";
const TOKEN: &str = "connector-private-service-token";

fn declaration() -> Value {
    json!({"id":"service","display":{"label":"Service"},"accounts":"single","origins":[ORIGIN],
        "auth":[{"kind":"secret","fields":["token"]}],
        "fields":[{"name":"token","title":"Token","secret":true}],
        "inject":[{"origin":ORIGIN,"header":"Authorization","prefix":"Bearer "}]})
}
fn configure(root: &Path, connector: Value) {
    let filename = root.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&filename).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","scope":"host","actions":["http.request"],"connectors":[connector]}});
    fs::write(filename, manifest.to_string()).unwrap();
}
async fn connected(
    client: &Client,
    package: plugin::Reference,
    connection: Option<connector::Id>,
    revision: Option<u64>,
    input: Input,
) -> connector::Connection {
    let request = client.prepare(Command::StartConnectorLogin {
        package,
        connector: "service".into(),
        connection,
        expected_revision: revision,
        method: Kind::Secret,
        input,
    });
    let Output::ConnectorLogin(attempt) = client.execute(request.clone()).await.unwrap() else {
        panic!("connector login expected");
    };
    assert_eq!(
        client.execute(request).await.unwrap(),
        Output::ConnectorLogin(attempt.clone())
    );
    let mut changes = client.subscribe_connector_login(attempt.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Update::ConnectorLogin(update) = changes.next().await.unwrap() else {
                panic!("connector update expected");
            };
            assert!(!serde_json::to_string(&update).unwrap().contains(TOKEN));
            match update.state {
                LoginState::Connected(connection) => return connection,
                state => assert!(state.active(), "unexpected state: {state:?}"),
            }
        }
    })
    .await
    .unwrap()
}
fn secret() -> Input {
    Input {
        secrets: BTreeMap::from([("token".into(), Secret::new(TOKEN.into()))]),
        ..Default::default()
    }
}
fn context(package: plugin::Reference, worktree: WorktreeId) -> Context {
    Context {
        package,
        worktree: Some(worktree),
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    }
}
fn request(path: &str, connection: connector::Id) -> Command {
    Command::RequestPluginHttp(http::Request {
        url: format!("{ORIGIN}{path}"),
        method: "GET".into(),
        headers: BTreeMap::new(),
        body: None,
        credential: None,
        connector: Some("service".into()),
        connection: Some(connection),
        idempotent: false,
        timeout_ms: 2000,
    })
}

#[tokio::test]
async fn preserves_private_credentials_and_durable_identity() {
    for remote in [false, true] {
        let server = server::Server::start().await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture(ORIGIN, &server.origin);
        configure(&directory.path().join("source/package"), declaration());
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let Output::ConnectorTypes(types) = execute(&client, Command::ListConnectorTypes).await
        else {
            panic!("connector types expected");
        };
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].declaration.id, "service");
        let mut feed = client.subscribe_connections().await.unwrap();
        let Update::ConnectionsChanged(initial) = feed.next().await.unwrap() else {
            panic!("connection snapshot expected");
        };
        assert!(initial.connections.is_empty());
        let row = connected(&client, package.clone(), None, None, secret()).await;
        assert_eq!(row.state, State::Connected);
        assert_eq!(row.configured, ["token"]);
        assert!(!serde_json::to_string(&row).unwrap().contains(TOKEN));
        let scope = context(package.clone(), worktree);
        let call = client
            .prepare(request("/ok", row.id))
            .with_plugin(scope.clone());
        let result = client.execute(call.clone()).await.unwrap();
        assert_eq!(client.execute(call).await.unwrap(), result);
        assert!(!serde_json::to_string(&result).unwrap().contains(TOKEN));
        {
            let records = server.requests.lock().unwrap();
            assert_eq!(records.len(), 1);
            assert!(
                records[0]
                    .0
                    .contains(&format!("authorization: Bearer {TOKEN}"))
            );
        }
        let mut override_header = request("/ok", row.id);
        if let Command::RequestPluginHttp(request) = &mut override_header {
            request
                .headers
                .insert("Authorization".into(), "override".into());
        }
        assert_eq!(
            client
                .execute(client.prepare(override_header).with_plugin(scope.clone()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let mut outside = request("/ok", row.id);
        if let Command::RequestPluginHttp(request) = &mut outside {
            request.url = "https://other.example.com/ok".into();
        }
        assert_eq!(
            client
                .execute(client.prepare(outside).with_plugin(scope.clone()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let Output::PluginHttp(redirect) = client
            .execute(
                client
                    .prepare(request("/redirect", row.id))
                    .with_plugin(scope.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("HTTP result expected");
        };
        assert_eq!(redirect.status, 302);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let Output::Connection(disabled) = execute(
            &client,
            Command::SetConnectionEnabled {
                connection: row.id,
                expected_revision: row.revision,
                enabled: false,
            },
        )
        .await
        else {
            panic!("connection expected");
        };
        assert!(!disabled.enabled);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(request("/ok", row.id))
                        .with_plugin(scope.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::DisconnectConnection {
                    connection: row.id,
                    expected_revision: row.revision
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::Connection(enabled) = execute(
            &client,
            Command::SetConnectionEnabled {
                connection: row.id,
                expected_revision: disabled.revision,
                enabled: true,
            },
        )
        .await
        else {
            panic!("connection expected");
        };
        let Output::Connection(disconnected) = execute(
            &client,
            Command::DisconnectConnection {
                connection: row.id,
                expected_revision: enabled.revision,
            },
        )
        .await
        else {
            panic!("connection expected");
        };
        assert_eq!(disconnected.state, State::NotConnected);
        assert!(disconnected.configured.is_empty());
        let relinked = connected(
            &client,
            package.clone(),
            Some(row.id),
            Some(disconnected.revision),
            secret(),
        )
        .await;
        assert_eq!(relinked.id, row.id);
        assert!(relinked.revision > disconnected.revision);
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(TOKEN));
        let Update::ConnectionsChanged(changed) =
            tokio::time::timeout(Duration::from_secs(5), feed.next())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("connection update expected");
        };
        assert!(changed.cursor > initial.cursor);
        assert!(!serde_json::to_string(&changed).unwrap().contains(TOKEN));
        execute(
            &client,
            Command::RemoveConnection {
                connection: row.id,
                expected_revision: relinked.revision,
            },
        )
        .await;
        let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
            panic!("connections expected");
        };
        assert!(rows.is_empty());
        drop(feed);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn probes_identity_on_both_controller_paths() {
    for remote in [false, true] {
        let server = server::Server::respond(
            Some(json!({"name":"Fixture account","id":"stable-account"}).to_string()),
            None,
        )
        .await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture(ORIGIN, &server.origin);
        let mut declared = declaration();
        declared["identity"] =
            json!({"request":format!("{ORIGIN}/identity"),"name":"/name","id":"/id"});
        declared["health"] = json!({"request":format!("{ORIGIN}/health"),"ok":[200]});
        configure(&directory.path().join("source/package"), declared);
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let row = connected(&client, package, None, None, secret()).await;
        assert_eq!(row.identity.as_ref().unwrap().name, "Fixture account");
        let Output::Connection(probed) =
            execute(&client, Command::ProbeConnection { connection: row.id }).await
        else {
            panic!("connection expected");
        };
        assert_eq!(probed.state, State::Connected);
        assert_eq!(probed.identity, row.identity);
        assert!(probed.checked_at_ms.is_some());
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn stops_timed_out_requests_without_replay() {
    for remote in [false, true] {
        let server = server::Server::start().await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture(ORIGIN, &server.origin);
        configure(&directory.path().join("source/package"), declaration());
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let row = connected(&client, package.clone(), None, None, secret()).await;
        let mut command = request("/hold", row.id);
        if let Command::RequestPluginHttp(input) = &mut command {
            input.timeout_ms = 100;
        }
        let request = client
            .prepare(command)
            .with_plugin(context(package, worktree));
        let task = tokio::spawn({
            let request = request.clone();
            async move {
                let result = client.execute(request).await;
                (client, result)
            }
        });
        tokio::time::timeout(Duration::from_secs(5), server.held.cancelled())
            .await
            .unwrap();
        let (client, result) = task.await.unwrap();
        let error = result.unwrap_err();
        assert!(!error.message.contains(TOKEN));
        tokio::time::timeout(Duration::from_secs(5), server.disconnected.cancelled())
            .await
            .unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert_eq!(client.execute(request).await.unwrap_err(), error);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
