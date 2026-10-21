use super::*;

fn connection(db: &rusqlite::Connection, id: plugin::connector::Id) -> Vec<u8> {
    db.query_row(
        "SELECT body FROM plugin_connections WHERE id=?1",
        [id.to_string()],
        |row| row.get(0),
    )
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
            let path = profile.join("storage/node.sqlite3");
            let db = rusqlite::Connection::open(&path).unwrap();
            let first = connection(&db, row.id);
            let second_id = plugin::connector::Id::new();
            let mut second: serde_json::Value = serde_json::from_slice(&first).unwrap();
            second["id"] = json!(second_id);
            second["secrets"]["token"] = json!("second-account-private-marker");
            if invalid {
                second["grant_revision"] = json!(0);
            }
            let second = serde_json::to_vec_pretty(&second).unwrap();
            db.execute("INSERT INTO plugin_connections(id,plugin,connector,revision,body) VALUES(?1,'figma','figma',1,?2)", rusqlite::params![second_id.to_string(), second]).unwrap();
            let current: String = db
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name='turn_connections'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let previous = current.replace(
                "PRIMARY KEY(turn,connection)",
                "PRIMARY KEY(turn,plugin,connector)",
            );
            assert_ne!(previous, current);
            db.execute_batch("ALTER TABLE turn_connections RENAME TO captured_connections")
                .unwrap();
            db.execute_batch(&previous).unwrap();
            db.execute_batch("INSERT INTO turn_connections(turn,plugin,connector,connection,revision) SELECT turn,plugin,connector,connection,revision FROM captured_connections; DROP TABLE captured_connections").unwrap();
            let captured = upgrades::capture(&db, turn);
            let plugins: Vec<u8> = db
                .query_row(
                    "SELECT plugins FROM turns WHERE id=?1",
                    [turn.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            db.close().unwrap();
            let original_file = std::fs::read(&path).unwrap();
            let before = upgrades::backups(&profile);
            let restarted = Node::start_empty(&profile).await;
            if invalid {
                assert!(restarted.is_err());
                assert_eq!(std::fs::read(&path).unwrap(), original_file);
                assert_eq!(upgrades::backups(&profile), before);
                let db = rusqlite::Connection::open_with_flags(
                    &path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .unwrap();
                assert_eq!(upgrades::capture(&db, turn), captured);
                assert_eq!(connection(&db, row.id), first);
                assert_eq!(connection(&db, second_id), second);
                let layout: String = db
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE name='turn_connections'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(layout, previous);
                assert_eq!(
                    db.query_row::<i64, _, _>(
                        "SELECT count(*) FROM sqlite_master WHERE name='turn_connections_owner'",
                        [],
                        |row| row.get(0)
                    )
                    .unwrap(),
                    0
                );
                db.close().unwrap();
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
            let db = rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            assert_eq!(upgrades::capture(&db, turn), captured);
            let retained: Vec<u8> = db
                .query_row(
                    "SELECT plugins FROM turns WHERE id=?1",
                    [turn.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(retained, plugins);
            assert_eq!(connection(&db, row.id), first);
            assert_eq!(connection(&db, second_id), second);
            let mut statement = db.prepare("SELECT connection,revision FROM turn_connections WHERE turn=?1 AND plugin='figma' AND connector='figma' ORDER BY connection").unwrap();
            let actual = statement
                .query_map([next.to_string()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let mut expected = vec![(row.id.to_string(), 1), (second_id.to_string(), 1)];
            expected.sort();
            assert_eq!(actual, expected);
            drop(statement);
            let keys = db
                .prepare(
                    "SELECT name FROM pragma_table_info('turn_connections') WHERE pk>0 ORDER BY pk",
                )
                .unwrap()
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(keys, ["turn", "connection"]);
            assert_eq!(db.query_row::<i64,_,_>("SELECT \"unique\" FROM pragma_index_list('turn_connections') WHERE name='turn_connections_owner'", [], |row| row.get(0)).unwrap(),0);
            db.close().unwrap();
            let after = upgrades::backups(&profile);
            assert_eq!(after.len(), before.len() + 1);
            let backup = rusqlite::Connection::open_with_flags(
                after.iter().find(|path| !before.contains(path)).unwrap(),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            assert_eq!(upgrades::capture(&backup, turn), captured);
            assert_eq!(connection(&backup, row.id), first);
            assert_eq!(connection(&backup, second_id), second);
            backup.close().unwrap();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start_empty(&profile).await.unwrap();
            assert_eq!(upgrades::backups(&profile), after);
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
