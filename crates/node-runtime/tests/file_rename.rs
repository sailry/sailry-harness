use std::{path::Path, time::Duration};

use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

async fn register(client: &Client, path: &Path) -> WorktreeId {
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Rename fixture".into(),
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

fn rename(worktree: WorktreeId, from: &str, to: &str) -> Command {
    Command::RenameEntry {
        worktree,
        from: from.into(),
        to: to.into(),
    }
}

#[tokio::test]
async fn shares_idempotent_rename() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let worktree = register(&local, &project).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    for (index, transport) in [node.local(), controller.handle().remote(address)]
        .into_iter()
        .enumerate()
    {
        let client = Client::new(transport.clone());
        let from = format!("source-{index}");
        let to = format!("资料 {index}");
        std::fs::create_dir(project.join(&from)).unwrap();
        std::fs::write(project.join(&from).join("文件.txt"), "keep").unwrap();
        let mut watch = transport.subscribe(Topic::Files(worktree)).await.unwrap();
        watch.next().await.unwrap();
        let request = client.prepare(rename(worktree, &from, &to));
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let expected = Output::EntryRenamed {
            from: from.clone(),
            to: to.clone(),
        };
        assert_eq!(admission.completion.await.unwrap().unwrap(), expected);
        assert!(!project.join(&from).exists());
        assert_eq!(
            std::fs::read_to_string(project.join(&to).join("文件.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), watch.next())
                .await
                .unwrap()
                .unwrap(),
            Update::FilesChanged {
                node: node.id(),
                worktree
            }
        );
        drop(watch);
        std::fs::create_dir(project.join(&from)).unwrap();
        std::fs::write(project.join(&from).join("sentinel"), "replacement").unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), expected);
        assert!(project.join(&from).join("sentinel").exists());
        let mut changed = request;
        changed.command = rename(worktree, &from, "different");
        assert_eq!(
            client.execute(changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client
                .execute(client.prepare(rename(worktree, &from, &to)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client
                .execute(client.prepare(rename(WorktreeId::new(), &from, "wrong-owner")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let mut wrong = client.prepare(rename(worktree, &from, "wrong-target"));
        wrong.target = NodeId([0; 32]);
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        for (source, target) in [
            ("", "root"),
            (&from, "../escape"),
            (".git", "metadata"),
            (&from, ".git"),
            (&from, "missing/entry"),
        ] {
            assert!(
                client
                    .execute(client.prepare(rename(worktree, source, target)))
                    .await
                    .is_err()
            );
        }
        assert!(project.join(&from).join("sentinel").exists());
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn protects_owned_roots() {
    let temp = tempfile::tempdir().unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let worktree = register(&local, temp.path()).await;
    let nested = temp.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    register(&local, &nested).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&local, &remote] {
        for path in [
            "nested",
            "node",
            "node/storage",
            "node/storage/node.sqlite3",
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(rename(worktree, path, "moved")))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            assert!(temp.path().join(path).exists());
        }
        std::fs::write(temp.path().join("node/sentinel"), "keep").unwrap();
        assert_eq!(
            client
                .execute(client.prepare(rename(worktree, "node/sentinel", "moved")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        std::fs::write(temp.path().join("file"), "keep").unwrap();
        assert_eq!(
            client
                .execute(client.prepare(rename(worktree, "file", "node/incoming")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert!(temp.path().join("file").exists());
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_uncertain_results() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let local = Client::new(node.local());
    let worktree = register(&local, &project).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut requests = Vec::new();
    for (index, client) in [&local, &remote].into_iter().enumerate() {
        let from = format!("source-{index}");
        let to = format!("target-{index}");
        std::fs::write(project.join(&from), "content").unwrap();
        let request = client.prepare(rename(worktree, &from, &to));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(project.join(&to).exists());
        std::fs::write(project.join(&from), "replacement").unwrap();
        requests.push(request);
    }
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    node.shutdown().await.unwrap();
    let node = Node::start(&profile).await.unwrap();
    let local = Client::new(node.local());
    let remote = Client::new(controller.handle().remote(node.link().address()));
    for (index, client) in [&local, &remote].into_iter().enumerate() {
        assert_eq!(
            client
                .execute(requests[index].clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read_to_string(project.join(format!("source-{index}"))).unwrap(),
            "replacement"
        );
        assert_eq!(
            std::fs::read_to_string(project.join(format!("target-{index}"))).unwrap(),
            "content"
        );
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
