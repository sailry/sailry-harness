use std::path::Path;

use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

async fn register(client: &Client, path: &Path) -> WorktreeId {
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Trash fixture".into(),
            path: path.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot
        .worktrees
        .iter()
        .find(|tree| tree.project == Some(project.id))
        .unwrap()
        .id
}

fn command(worktree: WorktreeId, path: &str) -> Command {
    Command::TrashEntry {
        worktree,
        path: path.into(),
    }
}

#[tokio::test]
async fn preserves_owned_roots_and_storage() {
    let temp = tempfile::tempdir().unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let worktree = register(&local, temp.path()).await;
    std::fs::create_dir(temp.path().join("nested")).unwrap();
    register(&local, &temp.path().join("nested")).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    std::fs::write(temp.path().join("node/sentinel"), "preserved").unwrap();
    for client in [&local, &remote] {
        for checked in [false, true] {
            let operation = |worktree, path: &str| {
                if checked {
                    Command::TrashFile {
                        worktree,
                        path: path.into(),
                        expected_revision: "0".repeat(64),
                        expected_stamp: "0".repeat(64),
                    }
                } else {
                    command(worktree, path)
                }
            };
            for path in [
                "nested",
                "node",
                "node/storage",
                "node/storage/node.sqlite3",
                "node/sentinel",
            ] {
                assert_eq!(
                    client
                        .execute(client.prepare(operation(worktree, path)))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::Conflict
                );
                assert!(temp.path().join(path).exists());
            }
            for path in ["", "../outside", ".git", "nested/.GIT/config", "missing"] {
                assert!(
                    client
                        .execute(client.prepare(operation(worktree, path)))
                        .await
                        .is_err()
                );
            }
            assert_eq!(
                client
                    .execute(client.prepare(operation(WorktreeId::new(), "node")))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            let mut request = client.prepare(operation(worktree, "node"));
            request.target = NodeId([0; 32]);
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::WrongTarget
            );
        }
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[cfg(target_os = "macos")]
#[path = "file_trash/native.rs"]
mod native;
