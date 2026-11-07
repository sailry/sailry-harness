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
    let (directory, node, controller, client, worktree) = support::empty().await;
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

#[tokio::test]
async fn downloads_session_images_with_scoped_access() {
    let (directory, node, controller, client, worktree) = support::empty().await;
    let root = directory.path().join("source");
    let mut context = install_actions(
        &client,
        &root.join("package"),
        worktree,
        0,
        &[Action::ReadFiles],
    )
    .await;
    let config = serde_json::from_value(json!({
        "provider":ProviderId::new(), "model":"fixture", "mode":"code",
        "permission":"ask", "credential":null
    }))
    .unwrap();
    let Output::Session(session) = execute(
        &client,
        Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(config),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let workspace = Path::new(
        &snapshot
            .worktrees
            .iter()
            .find(|entry| entry.id == session.worktree)
            .unwrap()
            .path,
    );
    let bytes = super::super::media::server::image();
    let path = "assets/generated/image.png";
    fs::create_dir_all(workspace.join("assets/generated")).unwrap();
    fs::write(workspace.join(path), &bytes).unwrap();
    context.worktree = Some(session.worktree);
    context.session = Some(session.id);
    for scoped in [false, true] {
        let request = client.prepare(Command::DownloadFile {
            worktree: session.worktree,
            path: path.into(),
        });
        let request = if scoped {
            request.with_plugin(context.clone())
        } else {
            request
        };
        let Output::FileDownload(download) = client.execute(request).await.unwrap() else {
            panic!("download expected")
        };
        assert_eq!(download.size, bytes.len() as u64);
        assert_eq!(download.revision, blake3::hash(&bytes).to_hex().to_string());
        let mut received = Vec::new();
        client
            .download(&download, &mut received, CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(received, bytes);
    }
    denied(
        &client,
        &context,
        Command::DownloadFile {
            worktree,
            path: "plugin.json".into(),
        },
    )
    .await;
    assert!(
        client
            .execute(
                client
                    .prepare(Command::DownloadFile {
                        worktree: session.worktree,
                        path: "../../../storage/sentinel".into(),
                    })
                    .with_plugin(context.clone())
            )
            .await
            .is_err()
    );

    let Output::Project(project) = execute(
        &client,
        Command::RegisterProject {
            name: "Profile parent".into(),
            path: directory.path().to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let parent = snapshot
        .worktrees
        .iter()
        .find(|entry| entry.project == Some(project.id))
        .unwrap()
        .id;
    let mut parent_context = context.clone();
    parent_context.worktree = Some(parent);
    parent_context.session = None;
    fs::write(directory.path().join("node/storage/sentinel"), "protected").unwrap();
    for path in [
        "node/storage/sentinel".to_owned(),
        format!("node/workspaces/sessions/{}/{path}", session.id),
    ] {
        denied(
            &client,
            &parent_context,
            Command::DownloadFile {
                worktree: parent,
                path,
            },
        )
        .await;
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}
