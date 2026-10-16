use super::*;
use serde_json::json;

async fn package(client: &Client, root: &Path, worktree: WorktreeId) -> Context {
    let manifest = root.join("plugin.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["extensions"] = json!({"dev.sailry.platform":{
        "api_version":"v1", "actions":[Action::ReadFiles, Action::WriteFiles],
        "host":{"entry":"dev.sailry.platform/main.js", "resources":["dev.sailry.platform/main.js"], "handlers":["run"]}
    }});
    fs::write(manifest, value.to_string()).unwrap();
    fs::write(root.join("dev.sailry.platform/main.js"), r#"
        import {readFile, prepareFile, completeRequest} from 'sailry/sdk';
        export async function run(input) {
            if (input.write) return await completeRequest(prepareFile(input.path, 'Forbidden', null));
            try { return await readFile(input.path); }
            catch (error) { return {code:error.code}; }
        }
    "#).unwrap();
    let package = info(execute(client, install(worktree, 0)).await);
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    Context {
        package: package.summary.reference(),
        worktree: Some(worktree),
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    }
}

#[tokio::test]
async fn separates_manual_and_callback_access() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let root = directory.path().join("source");
        let base = ".sailry/worktrees/fixture";
        fs::create_dir_all(root.join(base)).unwrap();
        let path = format!("{base}/document.txt");
        let original = "Original document\n".repeat(MAX_FILE_BYTES / 8);
        fs::write(root.join(&path), &original).unwrap();
        let context = package(&client, &root.join("package"), worktree).await;
        let content = client
            .read_document_scoped(&context, &path, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(content.text, original);
        let revision = content.revision.unwrap();
        let staged = client
            .stage_document_scoped(
                &context,
                &path,
                "Manual save",
                &revision,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(staged.request.plugin.as_ref(), Some(&context));
        assert!(matches!(
            client.execute(staged.request).await.unwrap(),
            Output::FileWritten(_)
        ));
        assert_eq!(fs::read_to_string(root.join(&path)).unwrap(), "Manual save");
        let copy = format!("{base}/copy.txt");
        client
            .execute(
                client
                    .prepare(Command::CopyEntry {
                        worktree,
                        from: path.clone(),
                        to: copy.clone(),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap();
        for write in [false, true] {
            let requested = if write {
                format!("{base}/forbidden.txt")
            } else {
                path.clone()
            };
            let Output::PluginResult(result) = client
                .execute(
                    client
                        .prepare(Command::CallPlugin {
                            handler: "run".into(),
                            input: json!({"path":requested,"write":write}),
                        })
                        .with_plugin(context.clone()),
                )
                .await
                .unwrap()
            else {
                panic!("callback result expected")
            };
            if write {
                assert_eq!(result["Err"]["code"], "permission_denied");
            } else {
                assert_eq!(result["code"], "permission_denied");
            }
        }
        assert!(!root.join(base).join("forbidden.txt").exists());
        let other = other_worktree(&client, &directory.path().join("other")).await;
        denied(
            &client,
            &context,
            Command::ReadFile {
                worktree: other,
                path: path.clone(),
            },
        )
        .await;
        let mut invoked = context.clone();
        invoked.invocation = Some(RequestId::new());
        denied(
            &client,
            &invoked,
            Command::ReadFile {
                worktree,
                path: path.clone(),
            },
        )
        .await;
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadFile {
                            worktree,
                            path: path.clone(),
                        })
                        .with_plugin(context.clone()),
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(fs::read_to_string(root.join(copy)).unwrap(), "Manual save");
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
