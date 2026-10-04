use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::{path::PathBuf, time::Duration};

#[path = "support/files.rs"]
mod fixture;
use fixture::{Fixture, register};

#[path = "file_move/protection.rs"]
mod protection;

#[cfg(target_os = "macos")]
#[path = "file_move/cross_filesystem.rs"]
mod cross_filesystem;

fn move_to(source: WorktreeId, worktree: WorktreeId, from: &str, to: &str) -> Command {
    Command::MoveEntryTo {
        source,
        worktree,
        from: from.into(),
        to: to.into(),
    }
}

async fn destination(fixture: &Fixture) -> (PathBuf, WorktreeId) {
    let root = fixture._temp.path().join("destination");
    std::fs::create_dir(&root).unwrap();
    let worktree = register(&fixture.clients()[0], &root).await;
    (root, worktree)
}

#[tokio::test]
async fn moves_across_worktrees_once() {
    let fixture = Fixture::start().await;
    let (destination, worktree) = destination(&fixture).await;
    let clients = fixture.clients();
    let repository = git2::Repository::init(&fixture.root).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let tree = repository
        .find_tree(repository.index().unwrap().write_tree().unwrap())
        .unwrap();
    let head = repository
        .commit(Some("HEAD"), &author, &author, "Fixture", &tree, &[])
        .unwrap();
    let Output::Snapshot(snapshot) = clients[0]
        .execute(clients[0].prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let project = snapshot
        .worktrees
        .iter()
        .find(|tree| tree.id == fixture.worktree)
        .unwrap()
        .project
        .unwrap();
    let Output::Worktree(linked) = clients[0]
        .execute(clients[0].prepare(Command::CreateWorktree {
            project,
            path: fixture._temp.path().join("linked").to_str().unwrap().into(),
            branch: "linked".into(),
            commit: head.to_string(),
        }))
        .await
        .unwrap()
    else {
        panic!("worktree expected")
    };
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
        for (root, target) in [
            (&destination, worktree),
            (&PathBuf::from(&linked.path), linked.id),
        ] {
            let name = format!("资料-{index}-{target}");
            std::fs::create_dir_all(fixture.root.join(&name).join("sub/empty")).unwrap();
            std::fs::write(fixture.root.join(&name).join("sub/文件.bin"), &bytes).unwrap();
            std::fs::write(fixture.root.join(&name).join("sub/empty-file"), []).unwrap();
            let metadata =
                std::fs::metadata(fixture.root.join(&name).join("sub/文件.bin")).unwrap();
            let mut source_watch = transport
                .subscribe(Topic::Files(fixture.worktree))
                .await
                .unwrap();
            let mut target_watch = transport.subscribe(Topic::Files(target)).await.unwrap();
            source_watch.next().await.unwrap();
            target_watch.next().await.unwrap();
            let request = client.prepare(move_to(fixture.worktree, target, &name, &name));
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let expected = Output::EntryMoved {
                from: name.clone(),
                to: name.clone(),
            };
            assert_eq!(admission.completion.await.unwrap().unwrap(), expected);
            assert!(!fixture.root.join(&name).exists());
            assert_eq!(
                std::fs::read(root.join(&name).join("sub/文件.bin")).unwrap(),
                bytes
            );
            assert!(root.join(&name).join("sub/empty").is_dir());
            assert_eq!(
                std::fs::metadata(root.join(&name).join("sub/empty-file"))
                    .unwrap()
                    .len(),
                0
            );
            let moved = std::fs::metadata(root.join(&name).join("sub/文件.bin")).unwrap();
            assert_eq!(moved.modified().unwrap(), metadata.modified().unwrap());
            assert_eq!(moved.permissions(), metadata.permissions());
            for (watch, worktree) in [
                (&mut source_watch, fixture.worktree),
                (&mut target_watch, target),
            ] {
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(5), watch.next())
                        .await
                        .unwrap()
                        .unwrap(),
                    Update::FilesChanged {
                        node: fixture.node.id(),
                        worktree
                    }
                );
            }
            drop(source_watch);
            drop(target_watch);
            std::fs::create_dir(fixture.root.join(&name)).unwrap();
            std::fs::write(fixture.root.join(&name).join("sentinel"), "replacement").unwrap();
            std::fs::write(root.join(&name).join("sub/文件.bin"), "external edit").unwrap();
            assert_eq!(client.execute(request.clone()).await.unwrap(), expected);
            let mut changed = request;
            changed.command = move_to(target, fixture.worktree, &name, &name);
            assert_eq!(
                client.execute(changed).await.unwrap_err().code,
                ErrorCode::Conflict
            );
            assert_eq!(
                client
                    .execute(client.prepare(move_to(fixture.worktree, target, &name, &name)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            assert!(fixture.root.join(&name).join("sentinel").exists());
            assert_eq!(
                std::fs::read_to_string(root.join(&name).join("sub/文件.bin")).unwrap(),
                "external edit"
            );
            let returned = format!("returned-{name}");
            client
                .execute(client.prepare(move_to(target, fixture.worktree, &name, &returned)))
                .await
                .unwrap();
            assert!(!root.join(&name).exists());
            assert!(fixture.root.join(&returned).join("sub/empty").is_dir());
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn restart_preserves_replacements() {
    let fixture = Fixture::start().await;
    let (destination, worktree) = destination(&fixture).await;
    let profile = fixture._temp.path().join("node");
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut requests = Vec::new();
    for (index, client) in fixture.clients().into_iter().enumerate() {
        let name = format!("file-{index}");
        std::fs::write(fixture.root.join(&name), "content").unwrap();
        let request = client.prepare(move_to(fixture.worktree, worktree, &name, &name));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(!fixture.root.join(&name).exists());
        assert_eq!(
            std::fs::read_to_string(destination.join(&name)).unwrap(),
            "content"
        );
        std::fs::write(fixture.root.join(&name), "replacement").unwrap();
        std::fs::write(destination.join(&name), "external edit").unwrap();
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
        let name = format!("file-{index}");
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(&name)).unwrap(),
            "replacement"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join(&name)).unwrap(),
            "external edit"
        );
    }
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
