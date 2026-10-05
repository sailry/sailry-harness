use crate::file_fixture::{Fixture, register};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::{path::PathBuf, time::Duration};

#[path = "file_copy/between.rs"]
mod between;

fn copy(worktree: WorktreeId, from: &str, to: &str) -> Command {
    Command::CopyEntry {
        worktree,
        from: from.into(),
        to: to.into(),
    }
}

#[tokio::test]
async fn shares_idempotent_copy() {
    let fixture = Fixture::start().await;
    let bytes = vec![0xff; 10 * 1024 * 1024];
    for (index, transport) in [
        fixture.node.local(),
        fixture
            .controller
            .handle()
            .remote(fixture.node.link().address()),
    ]
    .into_iter()
    .enumerate()
    {
        let client = Client::new(transport.clone());
        let source = format!("source-{index}");
        let target = format!("资料 {index}");
        std::fs::create_dir_all(fixture.root.join(&source).join("sub/empty")).unwrap();
        std::fs::create_dir(fixture.root.join(&source).join(".git")).unwrap();
        std::fs::write(fixture.root.join(&source).join(".git/config"), "private").unwrap();
        std::fs::write(fixture.root.join(&source).join("sub/文件.bin"), &bytes).unwrap();
        std::fs::write(fixture.root.join(&source).join("sub/empty-file"), []).unwrap();
        let mut watch = transport
            .subscribe(Topic::Files(fixture.worktree))
            .await
            .unwrap();
        watch.next().await.unwrap();
        let request = client.prepare(copy(fixture.worktree, &source, &target));
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let expected = Output::EntryCopied {
            from: source.clone(),
            to: target.clone(),
        };
        assert_eq!(admission.completion.await.unwrap().unwrap(), expected);
        assert_eq!(
            std::fs::read(fixture.root.join(&target).join("sub/文件.bin")).unwrap(),
            bytes
        );
        assert!(fixture.root.join(&target).join("sub/empty").is_dir());
        assert_eq!(
            std::fs::metadata(fixture.root.join(&target).join("sub/empty-file"))
                .unwrap()
                .len(),
            0
        );
        assert!(!fixture.root.join(&target).join(".git").exists());
        assert!(fixture.root.join(&source).join(".git/config").exists());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), watch.next())
                .await
                .unwrap()
                .unwrap(),
            Update::FilesChanged {
                node: fixture.node.id(),
                worktree: fixture.worktree
            }
        );
        drop(watch);
        std::fs::write(
            fixture.root.join(&target).join("sub/文件.bin"),
            "external edit",
        )
        .unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), expected);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(&target).join("sub/文件.bin")).unwrap(),
            "external edit"
        );
        let mut changed = request;
        changed.command = copy(fixture.worktree, &source, "another");
        assert_eq!(
            client.execute(changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client
                .execute(client.prepare(copy(fixture.worktree, &source, &target)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            std::fs::read(fixture.root.join(&source).join("sub/文件.bin")).unwrap(),
            bytes
        );
    }
    assert_eq!(std::fs::read_dir(&fixture.root).unwrap().count(), 4);
    fixture.close().await;
}

#[tokio::test]
async fn protects_owned_resources() {
    let fixture = Fixture::start().await;
    std::fs::write(fixture.root.join("file"), "keep").unwrap();
    std::fs::create_dir(fixture.root.join("nested")).unwrap();
    let clients = fixture.clients();
    register(&clients[0], &fixture.root.join("nested")).await;
    let enclosing = register(&clients[0], fixture._temp.path()).await;
    for client in &clients {
        for (from, to) in [
            ("", "root"),
            ("file", "../escape"),
            (".git", "metadata"),
            ("file", ".git"),
            ("file", "missing/entry"),
            ("nested", "copy"),
            ("file", "nested"),
        ] {
            assert!(
                client
                    .execute(client.prepare(copy(fixture.worktree, from, to)))
                    .await
                    .is_err()
            );
        }
        for path in ["node", "node/storage", "node/storage/node.sqlite3"] {
            assert_eq!(
                client
                    .execute(client.prepare(copy(enclosing, path, "copy")))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
        }
        assert_eq!(
            client
                .execute(client.prepare(copy(enclosing, "project/file", "node/incoming")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client
                .execute(client.prepare(copy(WorktreeId::new(), "file", "copy")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let mut wrong = client.prepare(copy(fixture.worktree, "file", "copy"));
        wrong.target = NodeId([0; 32]);
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        #[cfg(unix)]
        {
            let link = fixture.root.join("link");
            if !link.exists() {
                std::os::unix::fs::symlink("file", &link).unwrap();
            }
            assert!(
                client
                    .execute(client.prepare(copy(fixture.worktree, "link", "copy")))
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("file")).unwrap(),
            "keep"
        );
        assert!(!fixture.root.join("copy").exists());
    }
    fixture.close().await;
}

#[tokio::test]
async fn preserves_uncertain_result() {
    let fixture = Fixture::start().await;
    let profile = fixture._temp.path().join("node");
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut requests = Vec::new();
    for (index, client) in fixture.clients().into_iter().enumerate() {
        let from = format!("source-{index}");
        let to = format!("target-{index}");
        std::fs::write(fixture.root.join(&from), "contents").unwrap();
        let request = client.prepare(copy(fixture.worktree, &from, &to));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(&to)).unwrap(),
            "contents"
        );
        std::fs::write(fixture.root.join(&to), "external edit").unwrap();
        requests.push(request);
    }
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    fixture.node.shutdown().await.unwrap();
    let node = Node::start(profile).await.unwrap();
    for (index, client) in [
        Client::new(node.local()),
        Client::new(fixture.controller.handle().remote(node.link().address())),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            client
                .execute(requests[index].clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(format!("target-{index}"))).unwrap(),
            "external edit"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(format!("source-{index}"))).unwrap(),
            "contents"
        );
    }
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
