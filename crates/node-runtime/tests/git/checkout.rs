use super::*;

fn fixture(root: &Path) -> (Repository, git2::Oid, git2::Oid) {
    let repository = Repository::init(root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file"), "base\n").unwrap();
    std::fs::write(root.join("notes"), "notes\n").unwrap();
    commit(&repository);
    let base_id = repository.head().unwrap().target().unwrap();
    let feature_id = {
        let base = repository.find_commit(base_id).unwrap();
        let mut builder = repository.treebuilder(Some(&base.tree().unwrap())).unwrap();
        builder
            .insert("file", repository.blob(b"feature\n").unwrap(), 0o100644)
            .unwrap();
        let tree = repository.find_tree(builder.write().unwrap()).unwrap();
        let signature = Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let id = repository
            .commit(None, &signature, &signature, "Feature", &tree, &[&base])
            .unwrap();
        repository
            .branch("feature", &repository.find_commit(id).unwrap(), false)
            .unwrap();
        id
    };
    (repository, base_id, feature_id)
}

async fn request(client: &Client, worktree: WorktreeId, name: &str, commit: git2::Oid) -> Request {
    let before = status(client, worktree).await;
    client.prepare(Command::SwitchGitBranch {
        worktree,
        name: name.into(),
        commit: commit.to_string(),
        expected_head: before.head,
        expected_branch: before.branch,
        expected_index: before.index_revision.unwrap(),
    })
}

#[tokio::test]
async fn preserves_unrelated_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let (repository, base, feature) = fixture(&root);
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    std::fs::write(root.join("notes"), "staged notes\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("notes")).unwrap();
    index.write().unwrap();
    std::fs::write(root.join("notes"), "unstaged notes\n").unwrap();
    std::fs::write(root.join("untracked"), "keep").unwrap();
    for client in [&local, &remote] {
        let change = request(client, id, "feature", feature).await;
        let output = client.execute(change.clone()).await.unwrap();
        assert!(
            matches!(&output, Output::GitBranchSwitched(branch) if branch.name == "feature" && branch.current)
        );
        assert_eq!(
            repository.head().unwrap().name().unwrap(),
            "refs/heads/feature"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file")).unwrap(),
            "feature\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("notes")).unwrap(),
            "unstaged notes\n"
        );
        let index = repository.index().unwrap();
        let note = index.get_path(Path::new("notes"), 0).unwrap();
        assert_eq!(
            repository.find_blob(note.id).unwrap().content(),
            b"staged notes\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("untracked")).unwrap(),
            "keep"
        );
        client
            .execute(request(client, id, "main", base).await)
            .await
            .unwrap();
        assert_eq!(client.execute(change).await.unwrap(), output);
        assert_eq!(
            repository.head().unwrap().name().unwrap(),
            "refs/heads/main"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file")).unwrap(),
            "base\n"
        );
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_unsafe_switches() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let (repository, base, feature) = fixture(&root);
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    std::fs::write(root.join("file"), "user draft\n").unwrap();
    let change = request(&client, id, "feature", feature).await;
    assert_eq!(
        client.execute(change).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "user draft\n"
    );
    assert_eq!(repository.head().unwrap().target(), Some(base));
    std::fs::write(root.join("file"), "base\n").unwrap();
    std::fs::write(repository.path().join("index.lock"), "external writer").unwrap();
    assert_eq!(
        client
            .execute(request(&client, id, "feature", feature).await)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "base\n"
    );
    assert_eq!(
        std::fs::read_to_string(repository.path().join("index.lock")).unwrap(),
        "external writer"
    );
    std::fs::remove_file(repository.path().join("index.lock")).unwrap();
    let stale = request(&client, id, "feature", base).await;
    assert_eq!(
        client.execute(stale).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let stale = request(&client, id, "feature", feature).await;
    std::fs::write(root.join("notes"), "new staged\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("notes")).unwrap();
    index.write().unwrap();
    assert_eq!(
        client.execute(stale).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let reference = repository.find_reference("refs/heads/feature").unwrap();
    let mut options = git2::WorktreeAddOptions::new();
    options.reference(Some(&reference));
    repository
        .worktree("linked", &temp.path().join("linked"), Some(&options))
        .unwrap();
    assert_eq!(
        client
            .execute(request(&client, id, "feature", feature).await)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(repository.head().unwrap().target(), Some(base));
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "base\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("notes")).unwrap(),
        "new staged\n"
    );
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn lost_result_is_not_replayed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let (repository, _, feature) = fixture(&root);
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    let change = request(&client, id, "feature", feature).await;
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    assert_eq!(
        client.execute(change.clone()).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(repository.head().unwrap().target(), Some(feature));
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
        std::fs::read_to_string(root.join("file")).unwrap(),
        "after interruption\n"
    );
    node.shutdown().await.unwrap();
}
