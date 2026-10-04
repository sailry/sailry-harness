use super::*;

async fn list(client: &Client, worktree: WorktreeId) -> GitBranches {
    let request = client.prepare(Command::ListGitBranches { worktree });
    assert!(!request.command.durable());
    let Output::GitBranches(branches) = client.execute(request).await.unwrap() else {
        panic!()
    };
    branches
}

#[tokio::test]
async fn local_and_remote_creation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file"), "base").unwrap();
    commit(&repository);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("feature/中文", &base, false).unwrap();
    repository
        .reference("refs/remotes/origin/main", base.id(), false, "fixture")
        .unwrap();
    repository
        .reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            false,
            "fixture",
        )
        .unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let initial = list(&local, id).await;
    assert_eq!(list(&remote, id).await, initial);
    assert_eq!(initial.current.as_deref(), Some("main"));
    assert_eq!(
        initial
            .entries
            .iter()
            .map(|branch| (branch.name.as_str(), branch.remote, branch.current))
            .collect::<Vec<_>>(),
        vec![
            ("feature/中文", false, false),
            ("main", false, true),
            ("origin/main", true, false)
        ]
    );
    std::fs::write(root.join("file"), "unsaved worktree").unwrap();
    let index_before = std::fs::read(repository.path().join("index")).unwrap();
    for (index, client) in [&local, &remote].into_iter().enumerate() {
        let name = format!("new-{index}");
        let request = client.prepare(Command::CreateGitBranch {
            worktree: id,
            name: name.clone(),
            commit: base.id().to_string(),
        });
        assert!(request.command.durable());
        let created = client.execute(request.clone()).await.unwrap();
        assert!(
            matches!(&created, Output::GitBranchCreated(branch) if branch.name == name && branch.commit == base.id().to_string() && !branch.current)
        );
        repository
            .find_branch(&name, git2::BranchType::Local)
            .unwrap()
            .delete()
            .unwrap();
        assert_eq!(client.execute(request).await.unwrap(), created);
        assert!(
            repository
                .find_branch(&name, git2::BranchType::Local)
                .is_err()
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::CreateGitBranch {
                    worktree: id,
                    name: "main".into(),
                    commit: base.id().to_string()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        for (name, commit) in [
            ("bad name", base.id().to_string()),
            ("HEAD", base.id().to_string()),
            ("new", "main".into()),
            ("new", "0".repeat(40)),
        ] {
            assert!(
                client
                    .execute(client.prepare(Command::CreateGitBranch {
                        worktree: id,
                        name: name.into(),
                        commit
                    }))
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            repository.head().unwrap().name().unwrap(),
            "refs/heads/main"
        );
        assert_eq!(
            std::fs::read(repository.path().join("index")).unwrap(),
            index_before
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file")).unwrap(),
            "unsaved worktree"
        );
    }
    repository.set_head_detached(base.id()).unwrap();
    let detached = list(&remote, id).await;
    assert!(detached.current.is_none());
    assert!(detached.entries.iter().all(|branch| !branch.current));
    let reference = repository
        .branch("linked", &base, false)
        .unwrap()
        .into_reference();
    let mut options = git2::WorktreeAddOptions::new();
    options.reference(Some(&reference));
    let linked = temp.path().join("linked");
    repository
        .worktree("linked", &linked, Some(&options))
        .unwrap();
    let linked_id = worktree(&remote, &linked).await;
    let linked_branches = list(&remote, linked_id).await;
    assert_eq!(linked_branches.current.as_deref(), Some("linked"));
    assert!(
        linked_branches
            .entries
            .iter()
            .any(|branch| branch.name == "linked" && branch.current)
    );
    assert!(
        linked_branches
            .entries
            .iter()
            .any(|branch| branch.name == "main" && !branch.current)
    );
    let command = Command::CreateGitBranch {
        worktree: linked_id,
        name: "concurrent".into(),
        commit: base.id().to_string(),
    };
    let (left, right) = tokio::join!(
        local.execute(local.prepare(command.clone())),
        remote.execute(remote.prepare(command))
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(
        left.err().or(right.err()).unwrap().code,
        ErrorCode::Conflict
    );
    assert!(
        list(&local, id)
            .await
            .entries
            .iter()
            .any(|branch| branch.name == "concurrent")
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_and_bounded_lists() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    assert_eq!(list(&client, id).await.kind, RepositoryKind::Directory);
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    let unborn = list(&client, id).await;
    assert_eq!(unborn.kind, RepositoryKind::Unborn);
    assert_eq!(unborn.current.as_deref(), Some("main"));
    assert!(unborn.entries.is_empty());
    commit(&repository);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    for index in 0..MAX_GIT_ENTRIES {
        repository
            .branch(&format!("branch-{index:04}"), &base, false)
            .unwrap();
    }
    let bounded = list(&client, id).await;
    assert!(bounded.truncated);
    assert_eq!(bounded.entries.len(), MAX_GIT_ENTRIES);
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    drop(db);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn lost_result_is_not_replayed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    commit(&repository);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let request = client.prepare(Command::CreateGitBranch {
        worktree: id,
        name: "created".into(),
        commit: base.id().to_string(),
    });
    assert_eq!(
        client.execute(request.clone()).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert!(
        repository
            .find_branch("created", git2::BranchType::Local)
            .is_ok()
    );
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    node.shutdown().await.unwrap();
    repository
        .find_branch("created", git2::BranchType::Local)
        .unwrap()
        .delete()
        .unwrap();
    let node = Node::start(&profile).await.unwrap();
    assert_eq!(
        Client::new(node.local())
            .execute(request)
            .await
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    assert!(
        repository
            .find_branch("created", git2::BranchType::Local)
            .is_err()
    );
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn local_and_remote_unborn_creation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file"), "staged").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("file")).unwrap();
    index.write().unwrap();
    std::fs::write(root.join("file"), "unstaged").unwrap();
    let before = std::fs::read(repository.path().join("index")).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (visit, client) in [&local, &remote].into_iter().enumerate() {
        let snapshot = status(client, id).await;
        let command = |name: &str| Command::RunGitAction {
            worktree: id,
            action: GitAction::StartBranch { name: name.into() },
            expected_index: snapshot.index_revision.clone().unwrap(),
            expected_head: snapshot.head.clone(),
            expected_branch: snapshot.branch.clone(),
        };
        for name in ["bad..name", "--orphan", "HEAD", ""] {
            assert_eq!(
                client
                    .execute(client.prepare(command(name)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(
            client
                .execute(client.prepare(command(snapshot.branch.as_deref().unwrap())))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        let name = format!("feature/first-{visit}");
        let request = client.prepare(command(&name));
        assert_eq!(
            client.execute(request.clone()).await.unwrap(),
            Output::GitActionCompleted
        );
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::GitActionCompleted
        );
        assert_eq!(
            list(client, id).await.current.as_deref(),
            Some(name.as_str())
        );
        assert_eq!(
            repository.head().err().unwrap().code(),
            git2::ErrorCode::UnbornBranch
        );
        assert_eq!(
            client
                .execute(client.prepare(command("stale")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            std::fs::read(repository.path().join("index")).unwrap(),
            before
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file")).unwrap(),
            "unstaged"
        );
    }
    commit(&repository);
    let snapshot = status(&local, id).await;
    assert_eq!(
        local
            .execute(local.prepare(Command::RunGitAction {
                worktree: id,
                action: GitAction::StartBranch {
                    name: "too-late".into()
                },
                expected_index: snapshot.index_revision.unwrap(),
                expected_head: snapshot.head,
                expected_branch: snapshot.branch,
            }))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    drop(controller);
    node.shutdown().await.unwrap();
}
