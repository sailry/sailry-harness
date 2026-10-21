use super::*;
use sailry_protocol::{Command, Output, Secret, plugin::connector};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio::time::{Duration, timeout};

#[path = "../../../../node-runtime/tests/plugins/support.rs"]
#[allow(dead_code)]
mod support;

const TOKEN: &str = "bridge-private-connector-token";

async fn execute(connection: &Connection, command: Command) -> Output {
    let request = connection
        .prepare(serde_json::to_string(&command).unwrap())
        .unwrap();
    let response = connection.execute(request).await.unwrap();
    let response: sailry_link::Response = serde_json::from_str(&response).unwrap();
    response.unwrap()
}

async fn snapshot(
    updates: &ConnectionsUpdates,
    ready: impl Fn(&connector::Changes) -> bool,
) -> connector::Changes {
    timeout(Duration::from_secs(10), async {
        loop {
            let value = updates.next().await.unwrap();
            assert!(!value.contains(TOKEN));
            let snapshot: connector::Changes = serde_json::from_str(&value).unwrap();
            if ready(&snapshot) {
                return snapshot;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn observes_login_and_connection_snapshots() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let path = directory.path().join("source/package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","connectors":[{
            "id":"service","display":{"label":"Service"},"accounts":"single","origins":["https://service.example.com"],
            "auth":[{"kind":"secret","fields":["token"]}],
            "fields":[{"name":"token","title":"Token","secret":true}],
            "inject":[{"origin":"https://service.example.com","header":"Authorization","prefix":"Bearer "}]
        }]}});
        std::fs::write(path, manifest.to_string()).unwrap();
        let package = support::info(support::execute(&client, support::install(worktree, 0)).await)
            .summary
            .reference();
        let connection = Connection::new(
            if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            },
            CancellationToken::new(),
            Default::default(),
        );
        let updates = connection.watch_connections().await.unwrap();
        let initial = snapshot(&updates, |_| true).await;
        assert!(initial.connections.is_empty());
        assert_eq!(initial.types.len(), 1);
        assert_eq!(initial.types[0].package, package);
        let Output::ConnectorLogin(attempt) = execute(
            &connection,
            Command::StartConnectorLogin {
                package,
                connector: "service".into(),
                connection: None,
                expected_revision: None,
                method: connector::Kind::Secret,
                input: connector::Input {
                    secrets: BTreeMap::from([("token".into(), Secret::new(TOKEN.into()))]),
                    ..Default::default()
                },
            },
        )
        .await
        else {
            panic!("connector attempt expected")
        };
        let login = connection
            .watch_connector_login(serde_json::to_string(&attempt).unwrap())
            .await
            .unwrap();
        let connected = timeout(Duration::from_secs(10), async {
            loop {
                let value = login.next().await.unwrap();
                assert!(!value.contains(TOKEN));
                let view: Value = serde_json::from_str(&value).unwrap();
                assert!(view["error"].is_null());
                if view["update"].is_null() {
                    continue;
                }
                let update: connector::LoginUpdate =
                    serde_json::from_value(view["update"].clone()).unwrap();
                assert_eq!(update.attempt, attempt);
                if let connector::LoginState::Connected(row) = update.state {
                    break row;
                }
                assert!(update.state.active());
            }
        })
        .await
        .unwrap();
        let current = snapshot(&updates, |snapshot| {
            snapshot.connections.iter().any(|row| row == &connected)
        })
        .await;
        assert!(current.cursor > initial.cursor);
        assert_eq!(current.types, initial.types);
        assert_eq!(
            execute(&connection, Command::ListConnections).await,
            Output::Connections(vec![connected.clone()])
        );
        let Output::Connection(disabled) = execute(
            &connection,
            Command::SetConnectionEnabled {
                connection: connected.id,
                expected_revision: connected.revision,
                enabled: false,
            },
        )
        .await
        else {
            panic!("connection expected")
        };
        let changed = snapshot(&updates, |snapshot| {
            snapshot.connections.iter().any(|row| row == &disabled)
        })
        .await;
        assert!(changed.cursor > current.cursor);
        login.close();
        assert!(login.next().await.is_err());
        connection.close();
        assert!(updates.next().await.is_err());
        assert!(connection.watch_connections().await.is_err());
        assert_eq!(
            support::execute(&client, Command::ListConnections).await,
            Output::Connections(vec![disabled])
        );
        updates.close();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
