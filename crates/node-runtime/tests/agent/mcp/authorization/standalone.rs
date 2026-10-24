//! Identical endpoints remain separate resources under their explicit package owners.
use super::*;

#[tokio::test]
async fn removes_only_owned_authorization() {
    for remote in [false, true] {
        let peer = input::Server::url("https://example.invalid/continue", false).await;
        let issuer = issuer::Server::start(&peer.endpoint).await;
        let model = Server::start(false).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        fixture.node.set_connector_fixture(ORIGIN, &issuer.endpoint);
        let mut packages = Vec::new();
        let mut accounts = Vec::new();
        for name in ["mcp-first", "mcp-second"] {
            let Output::Plugin(info) = execute(
                &fixture.client,
                Command::InstallMcp {
                    name: name.into(),
                    expected_revision: 0,
                    definition: plugin::mcp::Definition::StreamableHttp {
                        url: format!("{ORIGIN}/mcp"),
                        headers: Vec::new(),
                    },
                    secrets: BTreeMap::new(),
                },
            )
            .await
            else {
                panic!("custom MCP expected")
            };
            assert_eq!(
                info.mcp[0].availability,
                plugin::mcp::Availability::Available
            );
            let Output::ConnectorLogin(attempt) = execute(
                &fixture.client,
                Command::StartConnectorLogin {
                    package: info.summary.reference(),
                    connector: "server".into(),
                    connection: None,
                    expected_revision: None,
                    method: Kind::McpOauth,
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
            let LoginState::Browser { url, .. } = state(&fixture.client, &attempt, |state| {
                matches!(state, LoginState::Browser { .. })
            })
            .await
            else {
                unreachable!()
            };
            accounts.push(complete(&fixture.client, &attempt, &url).await);
            packages.push(info);
        }
        assert_ne!(accounts[0].id, accounts[1].id);
        let (_, captured) = submit(&fixture, true).await;
        assert!(captured.plugins.contains(&packages[0].summary.reference()));
        let history = async || {
            fixture
                .node
                .with_storage(|db| {
                    Box::pin(async move {
                        let settings = db
                            .prepare("SELECT count(*) FROM plugin_settings")
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .query_row(())
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .get::<i64>(0)
                            .map_err(crate::database_fixture::fault)?;
                        let packages = db
                            .prepare("SELECT count(*) FROM plugin_packages")
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .query_row(())
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .get::<i64>(0)
                            .map_err(crate::database_fixture::fault)?;
                        let connections = db
                            .prepare("SELECT count(*) FROM turn_connections")
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .query_row(())
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .get::<i64>(0)
                            .map_err(crate::database_fixture::fault)?;
                        Ok((settings, packages, connections))
                    })
                })
                .await
                .unwrap()
        };
        let before = history().await;
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: packages[0].summary.name.clone(),
                expected_revision: packages[0].summary.revision,
            },
        )
        .await;
        assert_eq!(history().await, before);
        let Output::Connections(rows) = execute(&fixture.client, Command::ListConnections).await
        else {
            panic!("connections expected")
        };
        assert!(rows.iter().all(|row| row.id != accounts[0].id));
        assert_eq!(
            rows.iter().find(|row| row.id == accounts[1].id).unwrap(),
            &accounts[1]
        );
        let owned = fixture
            .node
            .with_storage(|db| {
                Box::pin(async move {
                    db.prepare("SELECT count(*) FROM plugin_connections WHERE plugin='mcp-first'")
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row(())
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .get::<i64>(0)
                        .map_err(crate::database_fixture::fault)
                })
            })
            .await
            .unwrap();
        assert_eq!(owned, 0);
        let sibling = fixture
            .node
            .with_storage(|db| {
                Box::pin(async move {
                    db.prepare("SELECT count(*) FROM plugin_connections WHERE plugin='mcp-second'")
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row(())
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .get::<i64>(0)
                        .map_err(crate::database_fixture::fault)
                })
            })
            .await
            .unwrap();
        assert_eq!(sibling, 1);
        assert!(model.requests.lock().unwrap().is_empty());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
