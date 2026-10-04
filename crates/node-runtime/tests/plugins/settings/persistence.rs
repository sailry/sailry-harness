use super::*;
use plugin::settings::{SecretUpdate, State};
use std::collections::BTreeMap;

pub(super) fn state(output: Output) -> State {
    let Output::PluginSettings(state) = output else {
        panic!("plugin settings expected")
    };
    state
}

pub(super) fn save(package: &plugin::Reference, endpoint: &str, update: SecretUpdate) -> Command {
    Command::SavePluginSettings {
        package: package.clone(),
        values: BTreeMap::from([("endpoint".into(), json!(endpoint))]),
        secrets: BTreeMap::from([("token".into(), update)]),
    }
}

pub(super) async fn read(client: &Client, package: &plugin::Reference) -> State {
    state(
        execute(
            client,
            Command::ReadPluginSettings {
                package: package.clone(),
            },
        )
        .await,
    )
}

#[tokio::test]
async fn recovers_saved_values() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        declare(&directory.path().join("source/package"), "Settings");
        let package = info(execute(&client, install(worktree, 0)).await);
        let initial = read(&client, &package.summary.reference()).await;
        assert!(!initial.ready);
        assert!(initial.configured.is_empty());
        assert_eq!(initial.values["endpoint"], "https://example.com");
        let secret = "fixture-private-token-${PLUGIN_ROOT}";
        let request = client.prepare(save(
            &initial.package,
            "https://one.example.com",
            SecretUpdate::Replace(Secret::new(secret.into())),
        ));
        assert!(!format!("{request:?}").contains(secret));
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let first = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break state(result.unwrap());
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(first.ready);
        assert_eq!(first.configured, ["token"]);
        assert_eq!(first.package.settings_revision, 1);
        assert!(!serde_json::to_string(&first).unwrap().contains(secret));
        assert_eq!(
            client.execute(request.clone()).await.unwrap(),
            Output::PluginSettings(first.clone())
        );
        let mut changed = request.clone();
        changed.command = save(&initial.package, "changed", SecretUpdate::Clear);
        assert_eq!(
            client.execute(changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client
                .execute(client.prepare(save(&initial.package, "stale", SecretUpdate::Clear)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let second = state(
            execute(
                &client,
                save(
                    &first.package,
                    "https://two.example.com",
                    SecretUpdate::Keep,
                ),
            )
            .await,
        );
        assert!(second.ready);
        assert_eq!(second.package.settings_revision, 2);
        let snapshot = execute(&client, Command::Snapshot).await;
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(secret));
        assert!(
            matches!(execute(&client, Command::ListCredentials).await, Output::Credentials(values) if values.is_empty())
        );
        let db = rusqlite::Connection::open_with_flags(
            node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let (body, result): (Vec<u8>, String) = db
            .query_row(
                "SELECT body,result FROM requests WHERE id=?1",
                [request.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(body.starts_with(b"credential-v1:"));
        assert!(!result.contains(secret));
        let mut query = db
            .prepare("SELECT body FROM plugin_settings WHERE plugin=?1 ORDER BY revision")
            .unwrap();
        let saved = query
            .query_map([&package.summary.name], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
            .map(|row| serde_json::from_slice::<serde_json::Value>(&row.unwrap()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(saved.len(), 2);
        for (saved, endpoint) in saved
            .iter()
            .zip(["https://one.example.com", "https://two.example.com"])
        {
            assert_eq!(saved["slots"]["token"], secret);
            assert_eq!(saved["values"]["endpoint"], endpoint);
        }
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='plugin_credentials'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0,
        );
        drop(query);
        drop(db);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(read(&client, &second.package).await, second);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::PluginSettings(first)
        );
        let cleared = state(
            execute(
                &client,
                save(
                    &second.package,
                    "https://two.example.com",
                    SecretUpdate::Clear,
                ),
            )
            .await,
        );
        assert!(!cleared.ready);
        assert!(cleared.configured.is_empty());
        let db = rusqlite::Connection::open_with_flags(
            node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM plugin_settings,json_each(plugin_settings.body,'$.slots') WHERE json_each.type='text'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        let mut query = db
            .prepare("SELECT body FROM plugin_settings WHERE plugin=?1")
            .unwrap();
        for row in query
            .query_map([&package.summary.name], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
        {
            assert!(!String::from_utf8(row.unwrap()).unwrap().contains(secret));
        }
        drop(query);
        drop(db);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_invalid_edits() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        declare(&directory.path().join("source/package"), "Settings");
        let package = info(execute(&client, install(worktree, 0)).await);
        let reference = package.summary.reference();
        let mut unknown_public = save(&reference, "value", SecretUpdate::Keep);
        if let Command::SavePluginSettings { values, .. } = &mut unknown_public {
            values.insert("token".into(), json!("must-not-be-public"));
        }
        let mut missing_intent = save(&reference, "value", SecretUpdate::Keep);
        if let Command::SavePluginSettings { secrets, .. } = &mut missing_intent {
            secrets.clear();
        }
        let mut missing_public = save(&reference, "value", SecretUpdate::Keep);
        if let Command::SavePluginSettings { values, .. } = &mut missing_public {
            values.clear();
        }
        let mut wrong_type = save(&reference, "value", SecretUpdate::Keep);
        if let Command::SavePluginSettings { values, .. } = &mut wrong_type {
            values.insert("endpoint".into(), json!(false));
        }
        for command in [
            unknown_public,
            missing_intent,
            missing_public,
            wrong_type,
            save(
                &reference,
                "value",
                SecretUpdate::Replace(Secret::new("".into())),
            ),
            save(
                &reference,
                "value",
                SecretUpdate::Replace(Secret::new("nul\0value".into())),
            ),
            save(
                &reference,
                "value",
                SecretUpdate::Replace(Secret::new("x".repeat(plugin::settings::MAX_STRING + 1))),
            ),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "example".into()
                    }
                )
                .await
            )
            .summary
            .reference(),
            reference
        );
        assert!(!read(&client, &reference).await.ready);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
