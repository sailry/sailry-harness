use super::{persistence::*, *};
use plugin::Context;

fn request(client: &Client, context: &Context) -> Request {
    client
        .prepare(Command::ReadPluginSettings {
            package: context.package.clone(),
        })
        .with_plugin(context.clone())
}

#[tokio::test]
async fn reads_public_values() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        declare(&directory.path().join("source/package"), "Settings");
        let package = info(execute(&client, install(worktree, 0)).await);
        assert!(package.extension.as_ref().unwrap().actions.is_empty());
        let saved = state(
            execute(
                &client,
                save(
                    &package.summary.reference(),
                    "https://公开.example.com",
                    plugin::settings::SecretUpdate::Replace(Secret::new(
                        "private-fixture-token".into(),
                    )),
                ),
            )
            .await,
        );
        let context = Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: saved.package.clone(),
            worktree: Some(worktree),
            session: None,
        };
        let request = request(&client, &context);
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(!admission.receipt.durable);
        let output = admission.completion.await.unwrap().unwrap();
        assert_eq!(state(output.clone()), saved);
        assert_eq!(saved.configured, ["token"]);
        assert!(
            !serde_json::to_string(&output)
                .unwrap()
                .contains("private-fixture-token")
        );
        assert!(!saved.values.contains_key("token"));
        let db = rusqlite::Connection::open_with_flags(
            node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM requests WHERE id=?1",
                [request.id.to_string()],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(db);
        let mut other = context.package.clone();
        other.name = "another-plugin".into();
        for command in [
            Command::ReadPluginSettings { package: other },
            save(
                &context.package,
                "unexpected",
                plugin::settings::SecretUpdate::Clear,
            ),
            Command::ListCredentials,
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        assert_eq!(read(&client, &saved.package).await, saved);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn checks_version_and_owner() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        declare(&directory.path().join("source/package"), "Settings");
        let package = info(execute(&client, install(worktree, 0)).await);
        let mut context = Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(worktree),
            session: None,
        };
        let stale = request(&client, &context);
        let saved = state(
            execute(
                &client,
                save(
                    &context.package,
                    "updated",
                    plugin::settings::SecretUpdate::Clear,
                ),
            )
            .await,
        );
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        context.package = saved.package.clone();
        let mut wrong = request(&client, &context);
        wrong.target = NodeId([99; 32]);
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        context.worktree = Some(WorktreeId::new());
        assert_eq!(
            client
                .execute(request(&client, &context))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        context.worktree = Some(worktree);
        context.session = Some(SessionId::new());
        assert_eq!(
            client
                .execute(request(&client, &context))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        context.session = None;
        assert_eq!(
            state(client.execute(request(&client, &context)).await.unwrap()),
            saved
        );
        let current = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "example".into(),
                },
            )
            .await,
        );
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: current.summary.revision,
                enabled: false,
            },
        )
        .await;
        assert_eq!(
            client
                .execute(request(&client, &context))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn saves_disabled_owned_revision() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        declare(&root, "Settings");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("plugin.json")).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["settings_page"] =
            serde_json::json!({"navigation":{"label":"Settings"}});
        manifest["extensions"]["dev.sailry.platform"]["host"] = json!({"entry":"dev.sailry.platform/main.js", "resources":["dev.sailry.platform/main.js"], "handlers":["run"]});
        fs::write(
            root.join("dev.sailry.platform/main.js"),
            "export function run() { return null; }",
        )
        .unwrap();
        fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
        let package = info(execute(&client, install(worktree, 0)).await);
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: package.summary.name.clone(),
                    expected_revision: package.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        let context = Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Settings,
            package: disabled.summary.reference(),
            worktree: None,
            session: None,
        };
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::CallPlugin {
                            handler: "run".into(),
                            input: json!(null),
                        })
                        .with_plugin(context.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured,
        );
        let request = client
            .prepare(save(
                &context.package,
                "https://settings.example",
                plugin::settings::SecretUpdate::Replace(Secret::new("fixture-token".into())),
            ))
            .with_plugin(context.clone());
        let saved = state(client.execute(request.clone()).await.unwrap());
        assert_eq!(state(client.execute(request).await.unwrap()), saved);
        assert_eq!(saved.values["endpoint"], "https://settings.example");
        let mut other = context.package.clone();
        other.name = "other".into();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(save(
                            &other,
                            "https://settings.example",
                            plugin::settings::SecretUpdate::Replace(Secret::new(
                                "fixture-token".into()
                            ))
                        ))
                        .with_plugin(context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
