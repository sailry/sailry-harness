use super::*;
use std::path::{Path, PathBuf};

pub(super) fn backups(profile: &Path) -> Vec<PathBuf> {
    let directory = profile.join("storage/backups");
    if !directory.exists() {
        return Vec::new();
    }
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .filter(|path| path.is_file())
        .collect()
}

pub(super) async fn capture(
    database: &turso::Connection,
    turn: TurnId,
) -> (String, String, String, i64) {
    let row = database
        .prepare("SELECT plugin,connector,connection,revision FROM turn_connections WHERE turn=?1")
        .await
        .unwrap()
        .query_row([turn.to_string()])
        .await
        .unwrap();
    (
        row.get(0).unwrap(),
        row.get(1).unwrap(),
        row.get(2).unwrap(),
        row.get(3).unwrap(),
    )
}

#[tokio::test]
async fn fills_fences_without_rewriting_captures() {
    for remote in [false, true] {
        for invalid in [false, true] {
            let resource = server::Server::with_json(json!({"document":{"id":"0:0"}}), None).await;
            let model = Server::tools(vec![(
                plugin_tool("figma", "read"),
                json!({"action":"file","file_key":"file-key"}),
            )])
            .await;
            let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
            install_figma(&fixture, &resource.origin, false).await;
            let turn = approvals::submit(&fixture.client, fixture.session.id).await;
            let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            process::decide(&fixture.client, &approval, Decision::Approve).await;
            finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(resource.requests.lock().unwrap().len(), 1);
            let Output::Connections(rows) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListConnections))
                .await
                .unwrap()
            else {
                panic!("connections expected")
            };
            let row = rows
                .iter()
                .find(|row| row.package.name == "figma")
                .unwrap()
                .clone();
            let profile = fixture.node.profile().to_owned();
            let id = fixture.node.id();
            fixture.node.shutdown().await.unwrap();
            fixture.controller.handle().disconnect(id).await;
            let path = profile.join("storage/node.turso");
            let (handle, mut connection) = crate::database_fixture::open(&path, false).await;
            let transaction = connection.transaction().await.unwrap();
            let database = &transaction;
            let bytes: Vec<u8> = (async {
                let mut statement = database
                    .prepare("SELECT body FROM plugin_connections WHERE id=?1")
                    .await?;
                let row = statement.query_row([row.id.to_string()]).await?;
                let mapped: turso::Result<_> = row.get(0);
                mapped
            })
            .await
            .unwrap();
            let mut stored: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            stored.as_object_mut().unwrap().remove("auth_revision");
            stored["revision"] = json!(7);
            if invalid {
                stored["grant_revision"] = json!(0);
            }
            let original = serde_json::to_vec_pretty(&stored).unwrap();
            database
                .execute(
                    "UPDATE plugin_connections SET revision=7,body=?1 WHERE id=?2",
                    turso::params![original.as_slice(), row.id.to_string()],
                )
                .await
                .unwrap();
            database
                .execute(
                    "UPDATE turn_connections SET revision=7 WHERE turn=?1",
                    [turn.to_string()],
                )
                .await
                .unwrap();
            database
                .execute_batch("DROP INDEX plugin_connections_owner")
                .await
                .unwrap();
            let captured = capture(database, turn).await;
            let plugins: Vec<u8> = (async {
                let mut statement = database
                    .prepare("SELECT plugins FROM turns WHERE id=?1")
                    .await?;
                let row = statement.query_row([turn.to_string()]).await?;
                let mapped: turso::Result<_> = row.get(0);
                mapped
            })
            .await
            .unwrap();
            transaction.commit().await.unwrap();
            crate::database_fixture::close(handle, connection).await;
            let original_file = std::fs::read(&path).unwrap();
            let existing = backups(&profile);
            let restarted = Node::start_empty(&profile).await;
            if invalid {
                assert!(restarted.is_err());
                assert_eq!(std::fs::read(&path).unwrap(), original_file);
                assert_eq!(backups(&profile), existing);
                let (handle, database) = crate::database_fixture::open(&path, true).await;
                assert_eq!(capture(&database, turn).await, captured);
                let retained: Vec<u8> = (async {
                    let mut statement = database
                        .prepare("SELECT body FROM plugin_connections WHERE id=?1")
                        .await?;
                    let row = statement.query_row([row.id.to_string()]).await?;
                    let mapped: turso::Result<_> = row.get(0);
                    mapped
                })
                .await
                .unwrap();
                assert_eq!(retained, original);
                assert_eq!(database.prepare("SELECT count(*) FROM sqlite_master WHERE name='plugin_connections_owner'").await.unwrap().query_row(()).await.unwrap().get::<i64>(0).unwrap(),0);
                crate::database_fixture::close(handle, database).await;
                fixture.controller.close().await.unwrap();
                continue;
            }
            fixture.node = restarted.unwrap();
            fixture
                .node
                .set_connector_fixture("https://api.figma.com", &resource.origin);
            fixture.client = Client::new(if remote {
                fixture
                    .controller
                    .handle()
                    .remote(fixture.node.link().address())
            } else {
                fixture.node.local()
            });
            {
                let expected_capture = captured.clone();
                let plugins = plugins.clone();
                let stored = stored.clone();
                let connection = row.id;
                fixture
                    .node
                    .with_storage(move |database| {
                        Box::pin(async move {
                            assert_eq!(capture(database, turn).await, expected_capture);
                            let retained: Vec<u8> = (async {
                                let mut statement = database
                                    .prepare("SELECT plugins FROM turns WHERE id=?1")
                                    .await?;
                                let row = statement.query_row([turn.to_string()]).await?;
                                let mapped: turso::Result<_> = row.get(0);
                                mapped
                            })
                            .await
                            .unwrap();
                            assert_eq!(retained, plugins);
                            let current: Vec<u8> = (async {
                                let mut statement = database
                                    .prepare("SELECT body FROM plugin_connections WHERE id=?1")
                                    .await?;
                                let row = statement.query_row([connection.to_string()]).await?;
                                let mapped: turso::Result<_> = row.get(0);
                                mapped
                            })
                            .await
                            .unwrap();
                            let mut current: serde_json::Value =
                                serde_json::from_slice(&current).unwrap();
                            assert_eq!(current["auth_revision"], 7);
                            current.as_object_mut().unwrap().remove("auth_revision");
                            assert_eq!(current, stored);
                            Ok(())
                        })
                    })
                    .await
                    .unwrap();
            }
            let backups = backups(&profile);
            assert_eq!(backups.len(), existing.len() + 1);
            let backup = backups
                .iter()
                .find(|path| !existing.contains(path))
                .unwrap();
            let (handle, previous) = crate::database_fixture::open(backup, true).await;
            let retained: Vec<u8> = (async {
                let mut statement = previous
                    .prepare("SELECT body FROM plugin_connections WHERE id=?1")
                    .await?;
                let row = statement.query_row([row.id.to_string()]).await?;
                let mapped: turso::Result<_> = row.get(0);
                mapped
            })
            .await
            .unwrap();
            assert_eq!(retained, original);
            assert_eq!(capture(&previous, turn).await, captured);
            crate::database_fixture::close(handle, previous).await;
            let Output::Connections(rows) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListConnections))
                .await
                .unwrap()
            else {
                panic!("connections expected")
            };
            assert_eq!(rows[0].revision, 7);
            assert_eq!(rows[0].state, State::Connected);
            let result = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::RequestPluginHttp(plugin::http::Request {
                            url: "https://api.figma.com/v1/files/after-upgrade".into(),
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
                            package: row.package,
                            worktree: Some(fixture.session.worktree),
                            session: None,
                            turn: None,
                            invocation: None,
                            surface: Default::default(),
                        }),
                )
                .await
                .unwrap();
            assert!(matches!(result,Output::PluginHttp(response) if response.status==200));
            assert_eq!(resource.requests.lock().unwrap().len(), 2);
            fixture.node.shutdown().await.unwrap();
            let node = Node::start_empty(&profile).await.unwrap();
            assert_eq!(self::backups(&profile), backups);
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
