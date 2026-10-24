use super::*;

async fn retained(db: &Connection, turn: TurnId) -> turso::Result<Vec<(String, Vec<u8>)>> {
    let mut rows = db.query("SELECT 'settings',body FROM plugin_settings WHERE plugin='computer' UNION ALL SELECT key,value FROM plugin_values WHERE name='computer' UNION ALL SELECT 'capture',plugins FROM turns WHERE id=?1 ORDER BY 1", [turn.to_string()]).await?;
    let mut retained = Vec::new();
    while let Some(row) = rows.next().await? {
        retained.push((row.get(0)?, row.get(1)?));
    }
    Ok(retained)
}

#[tokio::test]
async fn preserves_choices_and_captures() {
    for remote in [false, true] {
        for enabled in [false, true] {
            let (directory, node, controller, client, worktree) = fixture(remote).await;
            let expected_version = version(&client, "computer").await;
            assert!(
                semver::Version::parse(&expected_version).unwrap()
                    > semver::Version::parse("0.1.1").unwrap()
            );
            let previous = previous(directory.path(), &client, worktree, "computer", "0.1.1").await;
            let previous = info(
                execute(
                    &client,
                    Command::SetPluginEnabled {
                        name: "computer".into(),
                        expected_revision: previous.summary.revision,
                        enabled: true,
                    },
                )
                .await,
            );
            let (session, turn) = capture(&client, worktree).await;
            let previous = if enabled {
                previous
            } else {
                disable(&client, previous).await
            };
            let profile = directory.path().join("node");
            let path = profile.join("storage/node.turso");
            node.shutdown().await.unwrap();
            let previous = bundled(&path, previous).await;
            let original = {
                let (handle, db) = crate::plugins::database_fixture::open(&path, true).await;
                let value = retained(&db, turn).await.unwrap();
                crate::plugins::database_fixture::close(handle, db).await;
                value
            };
            assert_eq!(original.len(), 3);
            let packages: Vec<plugin::Reference> = serde_json::from_slice(
                &original.iter().find(|(key, _)| key == "capture").unwrap().1,
            )
            .unwrap();
            let captured = packages
                .iter()
                .find(|package| package.name == "computer")
                .unwrap()
                .clone();
            assert_eq!(captured.digest, previous.summary.digest);
            let frozen = profile.join("plugins/packages").join(&captured.digest);
            let manifest = fs::read(frozen.join("plugin.json")).unwrap();
            let resource = fs::read(frozen.join("skills/retained/SKILL.md")).unwrap();

            let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
                .await
                .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let current = info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "computer".into(),
                    },
                )
                .await,
            );
            assert_eq!(
                current.summary.version.as_deref(),
                Some(expected_version.as_str())
            );
            assert_eq!(current.origin, Some(plugin::Origin::Bundled));
            assert_eq!(current.summary.revision, previous.summary.revision + 1);
            assert_eq!(
                current.summary.settings_revision,
                previous.summary.settings_revision
            );
            assert_eq!(current.summary.enabled, enabled);
            assert_ne!(current.summary.digest, captured.digest);
            assert!(current.mcp.iter().any(|server| server.name == "desktop"));
            assert_eq!(
                node.with_storage(move |db| Box::pin(async move {
                    retained(db, turn)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)
                }))
                .await
                .unwrap(),
                original
            );
            let historical = info(
                execute(
                    &client,
                    Command::ReadPluginVersion {
                        package: captured.clone(),
                    },
                )
                .await,
            );
            assert_eq!(historical.summary.version.as_deref(), Some("0.1.1"));
            assert_eq!(historical.summary.digest, captured.digest);
            assert!(historical.mcp.is_empty());
            assert_eq!(fs::read(frozen.join("plugin.json")).unwrap(), manifest);
            assert_eq!(
                fs::read(frozen.join("skills/retained/SKILL.md")).unwrap(),
                resource
            );
            let Output::Session(current_session) = execute(
                &client,
                Command::ReadSession {
                    session: session.id,
                },
            )
            .await
            else {
                panic!("session expected");
            };
            assert_eq!(current_session.config, session.config);
            assert_eq!(current_session.revision, session.revision);

            execute(
                &client,
                Command::RemovePlugin {
                    name: "computer".into(),
                    expected_revision: current.summary.revision,
                },
            )
            .await;
            node.shutdown().await.unwrap();
            let removed: Vec<u8> = {
                let (handle, db) = crate::plugins::database_fixture::open(&path, true).await;
                let row = db
                    .prepare("SELECT body FROM plugins WHERE name='computer' AND installed=0")
                    .await
                    .unwrap()
                    .query_row(())
                    .await
                    .unwrap();
                let value: turso::Result<_> = row.get(0);
                let value = value.unwrap();
                crate::plugins::database_fixture::close(handle, db).await;
                value
            };
            let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
                .await
                .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadPlugin {
                        name: "computer".into()
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            let current_removed: Vec<u8> = {
                let parameters = ();
                let sql = "SELECT body FROM plugins WHERE name='computer' AND installed=0";
                node.with_storage(move |db| {
                    Box::pin(async move {
                        let row = db
                            .prepare(sql)
                            .await
                            .map_err(crate::plugins::database_fixture::fault)?
                            .query_row(parameters)
                            .await
                            .map_err(crate::plugins::database_fixture::fault)?;
                        let result: turso::Result<_> = row.get(0);
                        result.map_err(crate::plugins::database_fixture::fault)
                    })
                })
                .await
                .unwrap()
            };
            assert_eq!(current_removed, removed);
            assert_eq!(fs::read(frozen.join("plugin.json")).unwrap(), manifest);
            assert_eq!(
                fs::read(frozen.join("skills/retained/SKILL.md")).unwrap(),
                resource
            );
            assert_eq!(
                node.with_storage(move |db| Box::pin(async move {
                    retained(db, turn)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)
                }))
                .await
                .unwrap()
                .iter()
                .find(|(key, _)| key == "capture"),
                original.iter().find(|(key, _)| key == "capture")
            );
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}
