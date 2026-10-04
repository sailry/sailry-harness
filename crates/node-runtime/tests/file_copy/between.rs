use super::*;

fn copy(source: WorktreeId, worktree: WorktreeId, from: &str, to: &str) -> Command {
    Command::CopyEntryTo {
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
async fn shares_cross_worktree_copies() {
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
    std::fs::create_dir_all(fixture.root.join("资料/sub/empty")).unwrap();
    std::fs::write(fixture.root.join("资料/sub/文件.bin"), &bytes).unwrap();
    std::fs::write(fixture.root.join("资料/sub/empty-file"), []).unwrap();
    std::fs::write(fixture.root.join("资料/.git"), "gitdir: private").unwrap();
    for (index, client) in clients.iter().enumerate() {
        for (root, target) in [
            (&destination, worktree),
            (&PathBuf::from(&linked.path), linked.id),
        ] {
            let to = format!("copied-{index}");
            // Both worktrees contain the same relative name with different contents.
            std::fs::create_dir_all(root.join("资料")).unwrap();
            std::fs::write(root.join("资料/other"), "wrong source").unwrap();
            let transport = if index == 0 {
                fixture.node.local()
            } else {
                fixture
                    .controller
                    .handle()
                    .remote(fixture.node.link().address())
            };
            let mut watch = transport.subscribe(Topic::Files(target)).await.unwrap();
            watch.next().await.unwrap();
            let request = client.prepare(copy(fixture.worktree, target, "资料", &to));
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let expected = Output::EntryCopied {
                from: "资料".into(),
                to: to.clone(),
            };
            assert_eq!(admission.completion.await.unwrap().unwrap(), expected);
            assert_eq!(
                std::fs::read(root.join(&to).join("sub/文件.bin")).unwrap(),
                bytes
            );
            assert!(root.join(&to).join("sub/empty").is_dir());
            assert_eq!(
                std::fs::metadata(root.join(&to).join("sub/empty-file"))
                    .unwrap()
                    .len(),
                0
            );
            assert!(!root.join(&to).join(".git").exists());
            assert!(!root.join(&to).join("other").exists());
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), watch.next())
                    .await
                    .unwrap()
                    .unwrap(),
                Update::FilesChanged {
                    node: fixture.node.id(),
                    worktree: target
                }
            );
            drop(watch);
            std::fs::write(root.join(&to).join("sub/文件.bin"), "external edit").unwrap();
            assert_eq!(client.execute(request.clone()).await.unwrap(), expected);
            let mut changed = request;
            changed.command = copy(target, target, "资料", &to);
            assert_eq!(
                client.execute(changed).await.unwrap_err().code,
                ErrorCode::Conflict
            );
            assert_eq!(
                std::fs::read_to_string(root.join(&to).join("sub/文件.bin")).unwrap(),
                "external edit"
            );
            assert_eq!(
                client
                    .execute(client.prepare(copy(fixture.worktree, target, "资料", &to)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            let back = format!("returned-{target}-{index}");
            client
                .execute(client.prepare(copy(target, fixture.worktree, "资料/other", &back)))
                .await
                .unwrap();
            assert_eq!(
                std::fs::read_to_string(fixture.root.join(back)).unwrap(),
                "wrong source"
            );
        }
    }
    assert_eq!(
        std::fs::read(fixture.root.join("资料/sub/文件.bin")).unwrap(),
        bytes
    );
    assert!(fixture.root.join("资料/.git").is_file());
    fixture.close().await;
}

#[tokio::test]
async fn confines_both_roots() {
    let fixture = Fixture::start().await;
    let (destination, worktree) = destination(&fixture).await;
    let clients = fixture.clients();
    let enclosing = register(&clients[0], fixture._temp.path()).await;
    std::fs::create_dir(fixture.root.join("nested")).unwrap();
    register(&clients[0], &fixture.root.join("nested")).await;
    std::fs::create_dir(destination.join("nested")).unwrap();
    register(&clients[0], &destination.join("nested")).await;
    std::fs::write(fixture.root.join("file"), "source").unwrap();
    std::fs::write(destination.join("file"), "destination").unwrap();
    for client in &clients {
        for (source, target, from, to, code) in [
            (
                WorktreeId::new(),
                worktree,
                "file",
                "copy",
                ErrorCode::NotFound,
            ),
            (
                fixture.worktree,
                WorktreeId::new(),
                "file",
                "copy",
                ErrorCode::NotFound,
            ),
            (
                fixture.worktree,
                worktree,
                "nested",
                "copy",
                ErrorCode::Conflict,
            ),
            (
                fixture.worktree,
                worktree,
                "file",
                "nested",
                ErrorCode::Conflict,
            ),
            (
                enclosing,
                worktree,
                "node/storage/node.sqlite3",
                "copy",
                ErrorCode::Conflict,
            ),
            (
                fixture.worktree,
                enclosing,
                "file",
                "node/incoming",
                ErrorCode::Conflict,
            ),
            (
                fixture.worktree,
                worktree,
                "../file",
                "copy",
                ErrorCode::InvalidRequest,
            ),
            (
                fixture.worktree,
                worktree,
                "file",
                "../copy",
                ErrorCode::InvalidRequest,
            ),
            (
                fixture.worktree,
                worktree,
                ".git/config",
                "copy",
                ErrorCode::PermissionDenied,
            ),
            (
                fixture.worktree,
                worktree,
                "file",
                ".git/config",
                ErrorCode::PermissionDenied,
            ),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(copy(source, target, from, to)))
                    .await
                    .unwrap_err()
                    .code,
                code
            );
        }
        #[cfg(unix)]
        for (root, source, target) in [
            (&fixture.root, fixture.worktree, worktree),
            (&destination, worktree, fixture.worktree),
        ] {
            let link = root.join("link");
            if !link.exists() {
                std::os::unix::fs::symlink("file", &link).unwrap();
            }
            assert!(
                client
                    .execute(client.prepare(copy(source, target, "link", "copy")))
                    .await
                    .is_err()
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("file")).unwrap(),
        "source"
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("file")).unwrap(),
        "destination"
    );
    assert!(!fixture.root.join("copy").exists());
    assert!(!destination.join("copy").exists());
    fixture.close().await;
}

#[tokio::test]
async fn preserves_lost_completion() {
    let fixture = Fixture::start().await;
    let (destination, worktree) = destination(&fixture).await;
    let profile = fixture._temp.path().join("node");
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut requests = Vec::new();
    for (index, client) in fixture.clients().into_iter().enumerate() {
        let from = format!("source-{index}");
        let to = format!("copy-{index}");
        std::fs::write(fixture.root.join(&from), "contents").unwrap();
        let request = client.prepare(copy(fixture.worktree, worktree, &from, &to));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read_to_string(destination.join(&to)).unwrap(),
            "contents"
        );
        std::fs::write(destination.join(&to), "external edit").unwrap();
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
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
            std::fs::read_to_string(destination.join(format!("copy-{index}"))).unwrap(),
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
