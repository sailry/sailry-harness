use super::*;
use git2::Oid;

fn child(repository: &Repository, parent: Oid, name: &str, content: &str) -> Oid {
    let parent = repository.find_commit(parent).unwrap();
    let mut tree = repository
        .treebuilder(Some(&parent.tree().unwrap()))
        .unwrap();
    tree.insert(name, repository.blob(content.as_bytes()).unwrap(), 0o100644)
        .unwrap();
    let tree = repository.find_tree(tree.write().unwrap()).unwrap();
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(None, &author, &author, "Fixture", &tree, &[&parent])
        .unwrap()
}

fn fixture(root: &Path) -> (Repository, Oid, Oid) {
    let repository = Repository::init(root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file"), "base\n").unwrap();
    commit(&repository);
    let base = repository.head().unwrap().target().unwrap();
    let source = child(&repository, base, "file", "source\n");
    repository
        .branch("feature", &repository.find_commit(source).unwrap(), false)
        .unwrap();
    let mut config = repository.config().unwrap();
    config.set_str("user.name", "Fixture").unwrap();
    config
        .set_str("user.email", "fixture@example.invalid")
        .unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    (repository, base, source)
}

fn advance(repository: &Repository, id: Oid) {
    let tree = repository.find_commit(id).unwrap().tree().unwrap();
    repository
        .checkout_tree(
            tree.as_object(),
            Some(git2::build::CheckoutBuilder::new().safe()),
        )
        .unwrap();
    repository
        .find_reference("refs/heads/main")
        .unwrap()
        .set_target(id, "Advance fixture")
        .unwrap();
}

async fn request(client: &Client, worktree: WorktreeId, source: Oid) -> Request {
    let before = status(client, worktree).await;
    client.prepare(Command::MergeGitBranch {
        worktree,
        name: "feature".into(),
        commit: source.to_string(),
        expected_head: before.head.unwrap(),
        expected_branch: before.branch.unwrap(),
        expected_index: before.index_revision.unwrap(),
    })
}

#[tokio::test]
async fn local_and_remote_merges() {
    let temp = tempfile::tempdir().unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (route, client) in [("local", &local), ("remote", &remote)] {
        for kind in [GitMergeKind::FastForward, GitMergeKind::MergeCommit] {
            let root = temp.path().join(format!("{route}-{kind:?}"));
            let (repository, base, source) = fixture(&root);
            let target = if kind == GitMergeKind::MergeCommit {
                let id = child(&repository, base, "notes", "target notes\n");
                advance(&repository, id);
                id
            } else {
                base
            };
            let mut id = worktree(client, &root).await;
            // Exercise merging into a linked context without changing the primary checkout.
            let target_root = if route == "remote" {
                let linked = root.with_extension("linked");
                repository.worktree("target", &linked, None).unwrap();
                id = worktree(client, &linked).await;
                linked
            } else {
                root.clone()
            };
            let target_repo = Repository::open(&target_root).unwrap();
            let change = request(client, id, source).await;
            let admitted = client.dispatch(change.clone()).await.unwrap();
            assert!(admitted.receipt.durable);
            let output = admitted.completion.await.unwrap().unwrap();
            let Output::GitMerged(merged) = &output else {
                panic!("merge output expected")
            };
            assert_eq!(merged.kind, kind);
            let merged_id = Oid::from_str(&merged.commit).unwrap();
            assert_eq!(target_repo.head().unwrap().target(), Some(merged_id));
            assert_eq!(
                std::fs::read(target_root.join("file")).unwrap(),
                b"source\n"
            );
            assert!(target_repo.statuses(None).unwrap().is_empty());
            assert_eq!(target_repo.state(), git2::RepositoryState::Clean);
            if kind == GitMergeKind::MergeCommit {
                let commit = target_repo.find_commit(merged_id).unwrap();
                assert_eq!(commit.parent_ids().collect::<Vec<_>>(), [target, source]);
                assert_eq!(
                    std::fs::read(target_root.join("notes")).unwrap(),
                    b"target notes\n"
                );
            } else {
                assert_eq!(merged_id, source);
            }
            assert_eq!(
                repository
                    .find_reference("refs/heads/feature")
                    .unwrap()
                    .target(),
                Some(source)
            );
            if route == "remote" {
                assert_eq!(repository.head().unwrap().target(), Some(target));
                assert_eq!(std::fs::read(root.join("file")).unwrap(), b"base\n");
            }
            std::fs::write(target_root.join("draft"), "preserved").unwrap();
            assert_eq!(client.execute(change).await.unwrap(), output);
            let current = client
                .execute(request(client, id, source).await)
                .await
                .unwrap();
            assert_eq!(
                current,
                Output::GitMerged(GitMerge {
                    kind: GitMergeKind::UpToDate,
                    commit: merged.commit.clone()
                })
            );
            assert_eq!(
                std::fs::read(target_root.join("draft")).unwrap(),
                b"preserved"
            );
        }
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn conflicts_preserve_target() {
    let temp = tempfile::tempdir().unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (name, client) in [("local", &local), ("remote", &remote)] {
        let root = temp.path().join(name);
        let (repository, base, source) = fixture(&root);
        let id = worktree(client, &root).await;
        let stale = request(client, id, source).await;
        repository
            .find_reference("refs/heads/feature")
            .unwrap()
            .set_target(base, "Move source")
            .unwrap();
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        repository
            .find_reference("refs/heads/feature")
            .unwrap()
            .set_target(source, "Restore source")
            .unwrap();
        std::fs::write(root.join("file"), "draft\n").unwrap();
        assert_eq!(
            client
                .execute(request(client, id, source).await)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(std::fs::read(root.join("file")).unwrap(), b"draft\n");
        let stale = request(client, id, source).await;
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("file")).unwrap();
        index.write().unwrap();
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        // Restore the fixture's base file and index before constructing a conflicting target.
        std::fs::write(root.join("file"), "base\n").unwrap();
        index.add_path(Path::new("file")).unwrap();
        index.write().unwrap();
        let stale = request(client, id, source).await;
        let target = child(&repository, base, "file", "target\n");
        advance(&repository, target);
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let index = std::fs::read(repository.path().join("index")).unwrap();
        assert_eq!(
            client
                .execute(request(client, id, source).await)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(repository.head().unwrap().target(), Some(target));
        assert_eq!(std::fs::read(root.join("file")).unwrap(), b"target\n");
        assert_eq!(
            std::fs::read(repository.path().join("index")).unwrap(),
            index
        );
        assert_eq!(repository.state(), git2::RepositoryState::Clean);
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn locks_and_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let (repository, base, source) = fixture(&root);
    let target = child(&repository, base, "notes", "target notes\n");
    advance(&repository, target);
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
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
        repository
            .config()
            .unwrap()
            .set_bool("commit.gpgsign", true)
            .unwrap();
        assert_eq!(
            client
                .execute(request(client, id, source).await)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        repository
            .config()
            .unwrap()
            .set_bool("commit.gpgsign", false)
            .unwrap();
        let index = std::fs::read(repository.path().join("index")).unwrap();
        std::fs::write(repository.path().join("index.lock"), "external writer").unwrap();
        assert_eq!(
            client
                .execute(request(client, id, source).await)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        assert_eq!(
            std::fs::read(repository.path().join("index.lock")).unwrap(),
            b"external writer"
        );
        std::fs::remove_file(repository.path().join("index.lock")).unwrap();
        std::fs::write(repository.path().join("MERGE_HEAD"), format!("{source}\n")).unwrap();
        assert_eq!(
            client
                .execute(request(client, id, source).await)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        std::fs::remove_file(repository.path().join("MERGE_HEAD")).unwrap();
        let stale = request(client, id, source).await;
        repository.set_head_detached(target).unwrap();
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        repository.set_head("refs/heads/main").unwrap();
        assert_eq!(
            std::fs::read(repository.path().join("index")).unwrap(),
            index
        );
        assert_eq!(repository.head().unwrap().target(), Some(target));
        assert_eq!(std::fs::read(root.join("file")).unwrap(), b"base\n");
    }
    let left = request(&local, id, source).await;
    let right = request(&remote, id, source).await;
    let (left, right) = tokio::join!(local.execute(left), remote.execute(right));
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(
        left.err().or(right.err()).unwrap().code,
        ErrorCode::RevisionConflict
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn lost_result_is_not_replayed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let (repository, _, source) = fixture(&root);
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    let change = request(&client, id, source).await;
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    assert_eq!(
        client.execute(change.clone()).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(repository.head().unwrap().target(), Some(source));
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    node.shutdown().await.unwrap();
    std::fs::write(root.join("file"), "after interruption\n").unwrap();
    let node = Node::start(&profile).await.unwrap();
    assert_eq!(
        Client::new(node.local())
            .execute(change)
            .await
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        std::fs::read(root.join("file")).unwrap(),
        b"after interruption\n"
    );
    node.shutdown().await.unwrap();
}
