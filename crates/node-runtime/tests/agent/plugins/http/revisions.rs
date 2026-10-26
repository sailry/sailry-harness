use super::*;
use sailry_protocol::plugin::connector::{Connection, Input, Kind, LoginState, State};
#[path = "revisions/accounts.rs"]
mod accounts;
#[path = "revisions/upgrades.rs"]
mod upgrades;

async fn login(fixture: &process::Fixture, row: &Connection, input: Input) -> LoginState {
    let Output::ConnectorLogin(attempt) = fixture
        .client
        .execute(fixture.client.prepare(Command::StartConnectorLogin {
            package: row.package.clone(),
            connector: row.connector.clone(),
            connection: Some(row.id),
            expected_revision: Some(row.revision),
            method: Kind::Secret,
            input,
        }))
        .await
        .unwrap()
    else {
        panic!("login expected")
    };
    let mut updates = fixture
        .client
        .subscribe_connector_login(attempt.id)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Update::ConnectorLogin(update) = updates.next().await.unwrap() else {
                panic!("login update expected")
            };
            if !update.state.active() {
                return update.state;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn preserves_unchanged_edits_for_captured_tools() {
    for remote in [false, true] {
        for edit in ["unchanged", "public", "secret", "target"] {
            let changed = matches!(edit, "secret" | "target");
            let resource = server::Server::with_json(json!({"document":{"id":"0:0"}}), None).await;
            let model = Server::tools(vec![(
                plugin_tool("figma", "read"),
                json!({"action":"file","file_key":"file-key"}),
            )])
            .await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            install_figma(&fixture, &resource.origin, true).await;
            assert!(
                resource.requests.lock().unwrap()[0]
                    .0
                    .starts_with("GET /current/health ")
            );
            resource.requests.lock().unwrap().clear();
            let Output::Connections(rows) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListConnections))
                .await
                .unwrap()
            else {
                panic!("connections expected")
            };
            let row = rows.iter().find(|row| row.package.name == "figma").unwrap();
            let turn = approvals::submit(&fixture.client, fixture.session.id).await;
            let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            let captured = async || {
                let connection = row.id;
                fixture.node.with_storage(move |db| Box::pin(async move {
                    let row = db.prepare("SELECT plugin,connector,connection,revision FROM turn_connections WHERE turn=?1 AND connection=?2").await.map_err(crate::database_fixture::fault)?
                        .query_row(turso::params![turn.to_string(), connection.to_string()]).await.map_err(crate::database_fixture::fault)?;
                    Ok((row.get::<String>(0).map_err(crate::database_fixture::fault)?,row.get::<String>(1).map_err(crate::database_fixture::fault)?,row.get::<String>(2).map_err(crate::database_fixture::fault)?,row.get::<i64>(3).map_err(crate::database_fixture::fault)?))
                })).await.unwrap()
            };
            let before = captured().await;
            let Output::Connection(saved) = fixture
                .client
                .execute(
                    fixture.client.prepare(Command::UpdateConnection {
                        connection: row.id,
                        expected_revision: row.revision,
                        input: Input {
                            fields: match edit {
                                "public" => BTreeMap::from([("label".into(), "Renamed".into())]),
                                "target" => BTreeMap::from([("tenant".into(), "other".into())]),
                                _ => BTreeMap::new(),
                            },
                            secrets: BTreeMap::from([(
                                "token".into(),
                                Secret::new(
                                    if edit == "secret" {
                                        "replaced-private-marker"
                                    } else {
                                        PAT
                                    }
                                    .into(),
                                ),
                            )]),
                            ..Default::default()
                        },
                    }),
                )
                .await
                .unwrap()
            else {
                panic!("connection expected")
            };
            assert_eq!(saved.revision, row.revision + 1);
            assert_eq!(
                saved.state,
                if changed {
                    State::NotConnected
                } else {
                    State::Connected
                }
            );
            let saved = if changed {
                let LoginState::Connected(row) = login(&fixture, &saved, Input::default()).await
                else {
                    panic!("connected login expected")
                };
                {
                    let requests = resource.requests.lock().unwrap();
                    assert_eq!(requests.len(), 1);
                    assert!(requests[0].0.starts_with(if edit == "target" {
                        "GET /other/health "
                    } else {
                        "GET /current/health "
                    }));
                }
                resource.requests.lock().unwrap().clear();
                row
            } else {
                saved
            };
            process::decide(&fixture.client, &approval, Decision::Approve).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                resource.requests.lock().unwrap().len(),
                usize::from(!changed)
            );
            let result = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::RequestPluginHttp(plugin::http::Request {
                            url: "https://api.figma.com/v1/files/new-file".into(),
                            method: "GET".into(),
                            headers: BTreeMap::new(),
                            body: None,
                            credential: None,
                            connector: Some("figma".into()),
                            connection: Some(row.id),
                            idempotent: false,
                            timeout_ms: 2000,
                        }))
                        .with_plugin(plugin::Context {
                            package: saved.package,
                            worktree: Some(fixture.session.worktree),
                            session: None,
                            turn: None,
                            invocation: None,
                            surface: Default::default(),
                        }),
                )
                .await;
            assert!(
                matches!(result.unwrap(), Output::PluginHttp(response) if response.status == 200)
            );
            if !changed {
                assert!(
                    tool_details(&fixture.client, &page)
                        .await
                        .iter()
                        .any(|(_, detail)| {
                            let result = detail.result.as_ref().unwrap();
                            result["data"]["document"]["id"] == "0:0"
                        })
                );
            }
            assert_eq!(
                resource.requests.lock().unwrap().len(),
                if changed { 1 } else { 2 }
            );
            assert!(
                resource
                    .requests
                    .lock()
                    .unwrap()
                    .last()
                    .unwrap()
                    .0
                    .contains(&format!(
                        "x-figma-token: {}\r\n",
                        if edit == "secret" {
                            "replaced-private-marker"
                        } else {
                            PAT
                        }
                    ))
            );
            assert_eq!(captured().await, before);
            assert!(!serde_json::to_string(&page).unwrap().contains(PAT));
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn fences_changed_inputs_before_failed_relogin() {
    use std::sync::atomic::Ordering;
    for remote in [false, true] {
        for edit in ["secret", "target"] {
            let resource = server::Server::with_json(json!({"document":{"id":"0:0"}}), None).await;
            let model = Server::tools(vec![(
                plugin_tool("figma", "read"),
                json!({"action":"file","file_key":"captured-file"}),
            )])
            .await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            install_figma(&fixture, &resource.origin, true).await;
            let Output::Connections(rows) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListConnections))
                .await
                .unwrap()
            else {
                panic!("connections expected")
            };
            let original = rows.iter().find(|row| row.package.name == "figma").unwrap();
            let turn = approvals::submit(&fixture.client, fixture.session.id).await;
            let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            let captured = async || {
                let connection = original.id;
                fixture.node.with_storage(move |db| Box::pin(async move {
                    db.prepare("SELECT revision FROM turn_connections WHERE turn=?1 AND connection=?2").await.map_err(crate::database_fixture::fault)?
                        .query_row(turso::params![turn.to_string(), connection.to_string()]).await.map_err(crate::database_fixture::fault)?
                        .get::<i64>(0).map_err(crate::database_fixture::fault)
                })).await.unwrap()
            };
            let before = captured().await;
            resource.status.store(503, Ordering::SeqCst);
            let state = login(
                &fixture,
                original,
                Input {
                    fields: if edit == "target" {
                        BTreeMap::from([("tenant".into(), "other".into())])
                    } else {
                        BTreeMap::new()
                    },
                    secrets: if edit == "secret" {
                        BTreeMap::from([(
                            "token".into(),
                            Secret::new("relogin-private-marker".into()),
                        )])
                    } else {
                        BTreeMap::new()
                    },
                    ..Default::default()
                },
            )
            .await;
            assert!(
                matches!(state, LoginState::Failed(error) if error.code == ErrorCode::Unavailable)
            );
            let connection = original.id;
            let body: Vec<u8> = fixture
                .node
                .with_storage(move |db| {
                    Box::pin(async move {
                        db.prepare("SELECT body FROM plugin_connections WHERE id=?1")
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .query_row([connection.to_string()])
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .get(0)
                            .map_err(crate::database_fixture::fault)
                    })
                })
                .await
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["auth_revision"], original.revision + 1);
            assert!(body["grant"].is_null());
            assert_eq!(body["secrets"]["token"], PAT);
            assert_eq!(body["fields"]["tenant"], "current");
            assert_eq!(captured().await, before);
            resource.requests.lock().unwrap().clear();
            resource.status.store(0, Ordering::SeqCst);
            process::decide(&fixture.client, &approval, Decision::Approve).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert!(resource.requests.lock().unwrap().is_empty());
            let Output::Connections(rows) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListConnections))
                .await
                .unwrap()
            else {
                panic!("connections expected")
            };
            let failed = rows.iter().find(|row| row.id == original.id).unwrap();
            assert_eq!(failed.state, State::Connected);
            let LoginState::Connected(connected) = login(&fixture, failed, Input::default()).await
            else {
                panic!("connected login expected")
            };
            assert_eq!(connected.state, State::Connected);
            {
                let requests = resource.requests.lock().unwrap();
                assert_eq!(requests.len(), 1);
                assert!(requests[0].0.starts_with("GET /current/health "));
                assert!(requests[0].0.contains(&format!("x-figma-token: {PAT}\r\n")));
            }
            let request = fixture
                .client
                .prepare(Command::RequestPluginHttp(plugin::http::Request {
                    url: "https://api.figma.com/v1/files/new-file".into(),
                    method: "GET".into(),
                    headers: BTreeMap::new(),
                    body: None,
                    credential: None,
                    connector: Some("figma".into()),
                    connection: Some(original.id),
                    idempotent: false,
                    timeout_ms: 2000,
                }))
                .with_plugin(plugin::Context {
                    package: connected.package,
                    worktree: Some(fixture.session.worktree),
                    session: None,
                    turn: None,
                    invocation: None,
                    surface: Default::default(),
                });
            assert!(
                matches!(fixture.client.execute(request).await.unwrap(), Output::PluginHttp(response) if response.status == 200)
            );
            assert_eq!(resource.requests.lock().unwrap().len(), 2);
            assert_eq!(captured().await, before);
            let public = serde_json::to_string(&page).unwrap();
            for secret in [PAT, "relogin-private-marker"] {
                assert!(!public.contains(secret));
            }
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
