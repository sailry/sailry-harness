use super::*;

async fn connection(db: &turso::Connection, id: plugin::connector::Id) -> Vec<u8> {
    db.prepare("SELECT body FROM plugin_connections WHERE id=?1")
        .await
        .unwrap()
        .query_row([id.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap()
}

#[tokio::test]
async fn captures_each_account_after_atomic_upgrade() {
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
            let Output::Connections(rows) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListConnections))
                .await
                .unwrap()
            else {
                panic!("connections expected")
            };
            let row = rows.iter().find(|row| row.package.name == "figma").unwrap();
            let profile = fixture.node.profile().to_owned();
            let id = fixture.node.id();
            fixture.node.shutdown().await.unwrap();
            fixture.controller.handle().disconnect(id).await;
            let path = profile.join("storage/node.turso");
            let (handle, mut database) = crate::database_fixture::open(&path, false).await;
            let transaction = database.transaction().await.unwrap();
            let db = &transaction;
            let first = connection(db, row.id).await;
            let second_id = plugin::connector::Id::new();
            let mut second: serde_json::Value = serde_json::from_slice(&first).unwrap();
            second["id"] = json!(second_id);
            second["secrets"]["token"] = json!("second-account-private-marker");
            if invalid {
                second["grant_revision"] = json!(0);
            }
            let second = serde_json::to_vec_pretty(&second).unwrap();
            db.execute("INSERT INTO plugin_connections(id,plugin,connector,revision,body) VALUES(?1,'figma','figma',1,?2)", turso::params![second_id.to_string(), second.as_slice()]).await.unwrap();
            let current: String = (async {
                let mut statement = db
                    .prepare("SELECT sql FROM sqlite_master WHERE name='turn_connections'")
                    .await?;
                let row = statement.query_row(()).await?;
                let mapped: turso::Result<_> = row.get(0);
                mapped
            })
            .await
            .unwrap();
            let previous = current.replace(
                "PRIMARY KEY(turn,connection)",
                "PRIMARY KEY(turn,plugin,connector)",
            );
            assert_ne!(previous, current);
            db.execute_batch("ALTER TABLE turn_connections RENAME TO captured_connections")
                .await
                .unwrap();
            db.execute_batch(&previous).await.unwrap();
            db.execute_batch("INSERT INTO turn_connections(turn,plugin,connector,connection,revision) SELECT turn,plugin,connector,connection,revision FROM captured_connections; DROP TABLE captured_connections").await.unwrap();
            let captured = upgrades::capture(db, turn).await;
            let plugins: Vec<u8> = (async {
                let mut statement = db.prepare("SELECT plugins FROM turns WHERE id=?1").await?;
                let row = statement.query_row([turn.to_string()]).await?;
                let mapped: turso::Result<_> = row.get(0);
                mapped
            })
            .await
            .unwrap();
            transaction.commit().await.unwrap();
            crate::database_fixture::close(handle, database).await;
            let original_file = std::fs::read(&path).unwrap();
            let before = upgrades::backups(&profile);
            let restarted = Node::start_empty(&profile).await;
            if invalid {
                assert!(restarted.is_err());
                assert_eq!(std::fs::read(&path).unwrap(), original_file);
                assert_eq!(upgrades::backups(&profile), before);
                let (handle, db) = crate::database_fixture::open(&path, true).await;
                assert_eq!(upgrades::capture(&db, turn).await, captured);
                assert_eq!(connection(&db, row.id).await, first);
                assert_eq!(connection(&db, second_id).await, second);
                let layout: String = (async {
                    let mut statement = db
                        .prepare("SELECT sql FROM sqlite_master WHERE name='turn_connections'")
                        .await?;
                    let row = statement.query_row(()).await?;
                    let mapped: turso::Result<_> = row.get(0);
                    mapped
                })
                .await
                .unwrap();
                assert_eq!(layout, previous);
                assert_eq!(
                    db.prepare(
                        "SELECT count(*) FROM sqlite_master WHERE name='turn_connections_owner'"
                    )
                    .await
                    .unwrap()
                    .query_row(())
                    .await
                    .unwrap()
                    .get::<i64>(0)
                    .unwrap(),
                    0
                );
                crate::database_fixture::close(handle, db).await;
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
            let next = approvals::submit(&fixture.client, fixture.session.id).await;
            {
                let expected_capture = captured.clone();
                let plugins = plugins.clone();
                let first = first.clone();
                let second = second.clone();
                let account = row.id;
                fixture.node.with_storage(move |db| Box::pin(async move {
            assert_eq!(upgrades::capture(db, turn).await, expected_capture);
            let retained: Vec<u8> = (async { let mut statement = db.prepare("SELECT plugins FROM turns WHERE id=?1").await?;
let row = statement.query_row([turn.to_string()]).await?;
let mapped: turso::Result<_> = row.get(0);
mapped }).await
                .unwrap();
            assert_eq!(retained, plugins);
            assert_eq!(connection(db, account).await, first);
            assert_eq!(connection(db, second_id).await, second);
            let mut statement = db.prepare("SELECT connection,revision FROM turn_connections WHERE turn=?1 AND plugin='figma' AND connector='figma' ORDER BY connection").await.unwrap();
            let mut rows=statement.query([next.to_string()]).await.unwrap();
            let mut actual=Vec::new();
            while let Some(row)=rows.next().await.unwrap(){actual.push((row.get::<String>(0).unwrap(),row.get::<i64>(1).unwrap()));}
            drop(rows);
            let mut expected = vec![(account.to_string(), 1), (second_id.to_string(), 1)];
            expected.sort();
            assert_eq!(actual, expected);
            drop(statement);
            let mut rows=db.query("SELECT name FROM pragma_table_info('turn_connections') WHERE pk>0 ORDER BY pk",()).await.unwrap();
            let mut keys=Vec::new();
            while let Some(row)=rows.next().await.unwrap(){keys.push(row.get::<String>(0).unwrap());}
            drop(rows);
            assert_eq!(keys, ["turn", "connection"]);
            assert_eq!(db.prepare("SELECT \"unique\" FROM pragma_index_list('turn_connections') WHERE name='turn_connections_owner'").await.unwrap().query_row(()).await.unwrap().get::<i64>(0).unwrap(),0);
                    Ok(())
                })).await.unwrap();
            }
            let after = upgrades::backups(&profile);
            assert_eq!(after.len(), before.len() + 1);
            let (handle, backup) = crate::database_fixture::open(
                after.iter().find(|path| !before.contains(path)).unwrap(),
                true,
            )
            .await;
            assert_eq!(upgrades::capture(&backup, turn).await, captured);
            assert_eq!(connection(&backup, row.id).await, first);
            assert_eq!(connection(&backup, second_id).await, second);
            crate::database_fixture::close(handle, backup).await;
            fixture.node.shutdown().await.unwrap();
            let node = Node::start_empty(&profile).await.unwrap();
            assert_eq!(upgrades::backups(&profile), after);
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
