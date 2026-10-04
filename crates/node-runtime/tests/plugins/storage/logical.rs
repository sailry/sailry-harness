use super::*;

#[tokio::test]
async fn projects_and_validates_inline_schemas_on_both_paths() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        let original = actions::install_actions(
            &client,
            &root,
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        let old = write(
            &client,
            &original,
            "preferences",
            json!({"label":"Old","legacy":{"keep":true}}),
            0,
        )
        .await;
        write(&client, &original, "entry/one", json!({"label":"Entry"}), 0).await;
        let opaque = write(&client, &original, "opaque", json!([1, null, false]), 0).await;
        let path = root.join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let schema = json!({"type":"object","properties":{
            "label":{"type":"string"},"ready":{"type":"boolean","default":false}
        }});
        manifest["extensions"]["dev.sailry.platform"]["storage"] = json!({"collections":[
            {"scope":"node","key":"preferences","schema":schema},
            {"scope":"node","prefix":"entry/","schema":schema}
        ]});
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let installed = info(execute(&client, install(worktree, 1)).await);
        assert!(installed.issues.is_empty());
        let context = Context {
            package: installed.summary.reference(),
            ..original
        };
        let projected = read(&client, &context, "preferences").await;
        assert_eq!(projected.revision, old.revision);
        assert_eq!(
            projected.value,
            json!({"label":"Old","ready":false,"legacy":{"keep":true}})
        );
        assert_eq!(read(&client, &context, "preferences").await, projected);
        assert_eq!(
            read(&client, &context, "entry/one").await.value,
            json!({"label":"Entry","ready":false})
        );
        let missing = read(&client, &context, "entry/missing").await;
        assert_eq!(
            (missing.present, missing.revision, missing.value),
            (false, 0, Value::Null)
        );
        assert_eq!(read(&client, &context, "opaque").await, opaque);
        let saved = write(
            &client,
            &context,
            "preferences",
            json!({"label":"New"}),
            old.revision,
        )
        .await;
        assert_eq!(
            saved.value,
            json!({"label":"New","ready":false,"legacy":{"keep":true}})
        );
        assert_eq!(saved.revision, old.revision + 1);
        for (value, revision, code) in [
            (
                json!({"label":"Stale"}),
                old.revision,
                ErrorCode::RevisionConflict,
            ),
            (
                json!({"label":false}),
                saved.revision,
                ErrorCode::InvalidRequest,
            ),
        ] {
            assert_eq!(
                client
                    .execute(request(
                        &client,
                        &context,
                        Command::WritePluginValue {
                            key: "preferences".into(),
                            value,
                            expected_revision: revision,
                        }
                    ))
                    .await
                    .unwrap_err()
                    .code,
                code
            );
        }
        assert_eq!(read(&client, &context, "preferences").await, saved);
        let deleted = entry(
            run(
                &client,
                &context,
                Command::RemovePluginValue {
                    key: "preferences".into(),
                    expected_revision: saved.revision,
                },
            )
            .await,
        );
        assert!(!deleted.present);
        assert_eq!(read(&client, &context, "preferences").await, deleted);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
