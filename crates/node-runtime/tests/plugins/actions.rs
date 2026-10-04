use super::*;
use sailry_protocol::plugin::{Action, Context};
#[path = "actions/activity.rs"]
mod activity;
#[path = "actions/browser.rs"]
mod browser;
#[path = "actions/computer.rs"]
mod computer;
#[path = "actions/connections.rs"]
mod connections;
#[path = "actions/external_browser.rs"]
mod external_browser;
#[path = "actions/files.rs"]
mod files;
#[path = "actions/git.rs"]
mod git;
#[path = "actions/media.rs"]
mod media;
#[path = "actions/memory.rs"]
mod memory;
#[path = "actions/models.rs"]
mod models;
#[path = "actions/recovery.rs"]
mod recovery;
#[path = "actions/scopes.rs"]
mod scopes;
#[path = "actions/terminals.rs"]
mod terminals;
#[path = "actions/usage.rs"]
mod usage;
#[path = "actions/worktrees.rs"]
mod worktrees;

pub(super) fn declare(root: &Path, actions: &[Action]) {
    let path = root.join("plugin.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["extensions"] = serde_json::json!({
        "dev.sailry.platform": {"api_version": "v1", "actions": actions}
    });
    fs::write(path, manifest.to_string()).unwrap();
}

pub(super) async fn install_actions(
    client: &Client,
    root: &Path,
    worktree: WorktreeId,
    revision: u64,
    actions: &[Action],
) -> Context {
    declare(root, actions);
    let package = info(execute(client, install(worktree, revision)).await);
    assert_eq!(package.extension.as_ref().unwrap().actions, actions);
    Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: package.summary.reference(),
        worktree: Some(worktree),
        session: None,
    }
}

fn read(client: &Client, context: &Context) -> Request {
    client
        .prepare(Command::ReadFile {
            worktree: context.worktree.unwrap(),
            path: "notes.txt".into(),
        })
        .with_plugin(context.clone())
}

fn write(client: &Client, context: &Context, path: &str) -> Request {
    client
        .prepare(Command::WriteFile {
            worktree: context.worktree.unwrap(),
            path: path.into(),
            text: "完整结果 🙂".into(),
            expected_revision: None,
        })
        .with_plugin(context.clone())
}

#[tokio::test]
async fn uses_shared_handlers() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        git2::Repository::init(&root).unwrap();
        fs::write(root.join("notes.txt"), "Original 中文 🙂").unwrap();
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::ReadFiles, Action::WriteFiles, Action::ReadGit],
        )
        .await;
        let pending = client.dispatch(read(&client, &context)).await.unwrap();
        assert!(!pending.receipt.durable);
        let Output::FileContent(content) = pending.completion.await.unwrap().unwrap() else {
            panic!("file expected")
        };
        assert_eq!(content.text, "Original 中文 🙂");
        for command in [
            Command::ListDirectory {
                worktree,
                path: String::new(),
                after: None,
            },
            Command::InspectGit { worktree },
        ] {
            let expected = execute(&client, command.clone()).await;
            let result = client
                .execute(client.prepare(command).with_plugin(context.clone()))
                .await
                .unwrap();
            assert_eq!(result, expected);
        }
        let request = write(&client, &context, "saved.txt");
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break result.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(result, Output::FileWritten(_)));
        assert_eq!(
            fs::read_to_string(root.join("saved.txt")).unwrap(),
            "完整结果 🙂"
        );
        fs::write(root.join("saved.txt"), "External replacement").unwrap();
        let updated = install_actions(
            &client,
            &root.join("package"),
            worktree,
            1,
            &[Action::ReadFiles],
        )
        .await;
        assert_ne!(context.package.digest, updated.package.digest);
        assert_eq!(
            client
                .execute(read(&client, &context))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            client
                .execute(write(&client, &updated, "denied.txt"))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: 2,
                enabled: false,
            },
        )
        .await;
        assert_eq!(
            client
                .execute(read(&client, &updated))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        let mut different = request.clone();
        different.plugin = Some(updated);
        assert_eq!(
            client.execute(different.clone()).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client.outcome(&different).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(
            fs::read_to_string(root.join("saved.txt")).unwrap(),
            "External replacement"
        );
        assert!(!root.join("denied.txt").exists());
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_scope_escape() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::ReadFiles, Action::WriteFiles],
        )
        .await;
        let other_root = directory.path().join("other");
        fs::create_dir(&other_root).unwrap();
        let Output::Project(project) = execute(
            &client,
            Command::RegisterProject {
                name: "Other project".into(),
                path: other_root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let other = snapshot
            .worktrees
            .iter()
            .find(|entry| entry.project == Some(project.id))
            .unwrap()
            .id;
        for command in [
            Command::ReadFile {
                worktree: other,
                path: "notes.txt".into(),
            },
            Command::WriteFile {
                worktree: other,
                path: "outside.txt".into(),
                text: "Do not write".into(),
                expected_revision: None,
            },
            Command::Snapshot,
            Command::InspectHost,
            Command::CreateTerminal(terminal::Launch {
                worktree,
                viewport: terminal::Viewport {
                    columns: 80,
                    rows: 24,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                appearance: terminal::Appearance {
                    foreground: terminal::Rgb {
                        red: 255,
                        green: 255,
                        blue: 255,
                    },
                    background: terminal::Rgb {
                        red: 0,
                        green: 0,
                        blue: 0,
                    },
                    palette: [terminal::Rgb {
                        red: 128,
                        green: 128,
                        blue: 128,
                    }; 16],
                    color_scheme: terminal::ColorScheme::Dark,
                },
            }),
            Command::DownloadFile {
                worktree: other,
                path: "notes.txt".into(),
            },
            Command::ListCredentials,
            Command::InspectGit { worktree },
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: 1,
            },
        ] {
            let request = client.prepare(command).with_plugin(context.clone());
            let result = client.execute(request.clone()).await;
            assert_eq!(
                result.as_ref().unwrap_err().code,
                ErrorCode::PermissionDenied,
                "{request:?}: {result:?}"
            );
            if request.command.durable() {
                assert_eq!(
                    client.outcome(&request).await.unwrap(),
                    RequestOutcome::Completed(Box::new(result))
                );
            }
        }
        assert_eq!(
            execute(&client, Command::ListTerminals { worktree }).await,
            Output::Terminals(vec![])
        );
        assert!(!other_root.join("outside.txt").exists());
        for path in ["../other/outside.txt", "/outside.txt"] {
            assert!(
                client
                    .execute(write(&client, &context, path))
                    .await
                    .is_err()
            );
        }
        let mut missing = context.clone();
        missing.session = Some(SessionId::new());
        assert_eq!(
            client
                .execute(write(&client, &missing, "missing-session.txt"))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: 1,
            },
        )
        .await;
        assert_eq!(
            client
                .execute(read(&client, &context))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
