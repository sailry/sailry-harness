use super::*;
use persistence::{read, save, state};
use plugin::settings::SecretUpdate;

#[tokio::test]
async fn preserves_unset_defaults() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        declare(&root, "Settings");
        let path = root.join("dev.sailry.platform/settings.json");
        let mut schema: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        schema["properties"]["optional"] = json!({"type":"string","default":"Default value"});
        fs::write(&path, schema.to_string()).unwrap();
        let package = info(execute(&client, install(worktree, 0)).await);
        let initial = read(&client, &package.summary.reference()).await;
        assert_eq!(initial.values["optional"], "Default value");
        let saved = state(
            execute(
                &client,
                save(
                    &initial.package,
                    "Endpoint",
                    SecretUpdate::Replace(Secret::new("fixture-default-token".into())),
                ),
            )
            .await,
        );
        assert!(!saved.values.contains_key("optional"));
        assert!(saved.ready);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(read(&client, &saved.package).await, saved);
        schema["properties"]["added"] = json!({"type":"boolean","default":false});
        fs::write(&path, schema.to_string()).unwrap();
        let updated = info(
            execute(
                &client,
                install(worktree, inventory(&client).await[0].revision),
            )
            .await,
        );
        let changed = read(&client, &updated.summary.reference()).await;
        assert!(!changed.ready);
        assert!(!changed.values.contains_key("optional"));
        assert_eq!(changed.values["added"], false);
        let committed = state(
            execute(
                &client,
                Command::SavePluginSettings {
                    package: changed.package,
                    values: changed.values,
                    secrets: std::collections::BTreeMap::from([(
                        "token".into(),
                        SecretUpdate::Keep,
                    )]),
                },
            )
            .await,
        );
        assert!(committed.ready);
        assert!(!committed.values.contains_key("optional"));
        assert_eq!(committed.values["added"], false);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn requires_schema_save() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        declare(&root, "Settings");
        let first = info(execute(&client, install(worktree, 0)).await);
        let saved = state(
            execute(
                &client,
                save(
                    &first.summary.reference(),
                    "Original",
                    SecretUpdate::Replace(Secret::new("fixture-private-token".into())),
                ),
            )
            .await,
        );
        fs::write(root.join("content.txt"), "New package content").unwrap();
        let current = inventory(&client).await.remove(0);
        let second = info(execute(&client, install(worktree, current.revision)).await);
        assert_ne!(first.summary.digest, second.summary.digest);
        assert_eq!(
            second.summary.settings_revision,
            saved.package.settings_revision
        );
        let preserved = read(&client, &second.summary.reference()).await;
        assert_eq!(preserved.values, saved.values);
        assert_eq!(preserved.configured, saved.configured);
        assert!(preserved.ready);

        let mut schema: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("dev.sailry.platform/settings.json")).unwrap(),
        )
        .unwrap();
        schema["properties"]["endpoint"]["maxLength"] = json!(4);
        schema["properties"]["endpoint"]["default"] = json!("Next");
        schema["properties"]["token"]["x-sailry-secret"]["env"] = json!("OTHER");
        fs::write(
            root.join("dev.sailry.platform/settings.json"),
            schema.to_string(),
        )
        .unwrap();
        let mut mcp: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("mcp.json")).unwrap()).unwrap();
        mcp["mcpServers"]["service"]["env"]["OTHER"] = json!("");
        fs::write(root.join("mcp.json"), mcp.to_string()).unwrap();
        let third = info(execute(&client, install(worktree, second.summary.revision)).await);
        assert!(third.issues.is_empty());
        let changed = read(&client, &third.summary.reference()).await;
        assert!(!changed.ready);
        assert_eq!(changed.values, saved.values);
        assert!(changed.configured.is_empty());
        assert!(changed.keepable.is_empty());
        assert_eq!(
            client
                .execute(client.prepare(save(&changed.package, "Next", SecretUpdate::Keep)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let replaced = state(
            execute(
                &client,
                save(
                    &changed.package,
                    "Next",
                    SecretUpdate::Replace(Secret::new("replacement-token".into())),
                ),
            )
            .await,
        );
        assert!(replaced.ready);
        assert!(replaced.package.settings_revision > changed.package.settings_revision);
        let current = inventory(&client).await.remove(0);
        execute(
            &client,
            Command::SetPluginEnabled {
                name: current.name,
                expected_revision: current.revision,
                enabled: false,
            },
        )
        .await;
        let kept =
            state(execute(&client, save(&replaced.package, "Next", SecretUpdate::Keep)).await);
        assert!(kept.ready);
        let disabled = inventory(&client).await.remove(0);
        assert!(!disabled.enabled);
        execute(
            &client,
            Command::RemovePlugin {
                name: disabled.name,
                expected_revision: disabled.revision,
            },
        )
        .await;
        let reinstalled = info(execute(&client, install(worktree, 0)).await);
        assert_eq!(reinstalled.summary.settings_revision, 0);
        assert!(!read(&client, &reinstalled.summary.reference()).await.ready);
        let fresh = state(
            execute(
                &client,
                save(
                    &reinstalled.summary.reference(),
                    "Next",
                    SecretUpdate::Replace(Secret::new("reinstalled-token".into())),
                ),
            )
            .await,
        );
        assert!(fresh.package.settings_revision > kept.package.settings_revision);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
