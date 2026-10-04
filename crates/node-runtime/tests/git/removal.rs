use super::*;
use git2::{BranchType, Oid};

fn command(worktree: WorktreeId, name: &str, tip: Oid, head: Oid) -> Command {
    Command::DeleteGitBranch {
        worktree,
        name: name.into(),
        commit: tip.to_string(),
        expected_head: head.to_string(),
        expected_branch: Some("main".into()),
    }
}

#[tokio::test]
async fn local_and_remote_deletion() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file"), "base").unwrap();
    commit(&repository);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let head = repository
        .commit(
            Some("HEAD"),
            &author,
            &author,
            "Next",
            &base.tree().unwrap(),
            &[&base],
        )
        .unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    std::fs::write(root.join("file"), "uncommitted").unwrap();
    let index = std::fs::read(repository.path().join("index")).unwrap();
    for (index, client) in [&local, &remote].into_iter().enumerate() {
        let name = format!("feature/中文-{index}");
        repository.branch(&name, &base, false).unwrap();
        repository
            .config()
            .unwrap()
            .set_str(&format!("branch.{name}.remote"), "origin")
            .unwrap();
        repository
            .reference(
                &format!("refs/remotes/origin/{name}"),
                base.id(),
                false,
                "Fixture remote",
            )
            .unwrap();
        let request = client.prepare(command(id, &name, base.id(), head));
        assert!(request.command.durable());
        let output = client.execute(request.clone()).await.unwrap();
        assert_eq!(output, Output::GitBranchDeleted { name: name.clone() });
        assert!(repository.find_branch(&name, BranchType::Local).is_err());
        assert!(
            repository
                .config()
                .unwrap()
                .get_string(&format!("branch.{name}.remote"))
                .is_err()
        );
        assert!(
            repository
                .find_reference(&format!("refs/remotes/origin/{name}"))
                .is_ok()
        );
        repository.branch(&name, &base, false).unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        assert!(repository.find_branch(&name, BranchType::Local).is_ok());
    }
    assert_eq!(repository.head().unwrap().target(), Some(head));
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        index
    );
    assert_eq!(std::fs::read(root.join("file")).unwrap(), b"uncommitted");

    let reference = repository
        .branch("linked", &repository.find_commit(head).unwrap(), false)
        .unwrap()
        .into_reference();
    let linked = temp.path().join("linked");
    repository
        .worktree(
            "linked",
            &linked,
            Some(git2::WorktreeAddOptions::new().reference(Some(&reference))),
        )
        .unwrap();
    let linked_id = worktree(&remote, &linked).await;
    for client in [&local, &remote] {
        assert_eq!(
            client
                .execute(client.prepare(command(id, "linked", head, head)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::DeleteGitBranch {
                    worktree: linked_id,
                    name: "main".into(),
                    commit: head.to_string(),
                    expected_head: head.to_string(),
                    expected_branch: Some("linked".into()),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }
    repository.branch("from-linked", &base, false).unwrap();
    remote
        .execute(remote.prepare(Command::DeleteGitBranch {
            worktree: linked_id,
            name: "from-linked".into(),
            commit: base.id().to_string(),
            expected_head: head.to_string(),
            expected_branch: Some("linked".into()),
        }))
        .await
        .unwrap();
    assert!(
        repository
            .find_branch("from-linked", BranchType::Local)
            .is_err()
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_stale_and_unmerged_refs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    commit(&repository);
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let unmerged = repository
        .commit(
            Some("refs/heads/unmerged"),
            &author,
            &author,
            "Unmerged",
            &head.tree().unwrap(),
            &[&head],
        )
        .unwrap();
    repository.branch("merged", &head, false).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&local, &remote] {
        for (request, error) in [
            (
                command(id, "main", head.id(), head.id()),
                ErrorCode::Conflict,
            ),
            (
                command(id, "unmerged", unmerged, head.id()),
                ErrorCode::Conflict,
            ),
            (
                command(id, "merged", unmerged, head.id()),
                ErrorCode::RevisionConflict,
            ),
            (
                command(id, "merged", head.id(), unmerged),
                ErrorCode::RevisionConflict,
            ),
            (
                command(id, "missing", head.id(), head.id()),
                ErrorCode::NotFound,
            ),
            (
                command(WorktreeId::new(), "merged", head.id(), head.id()),
                ErrorCode::NotFound,
            ),
            (
                command(id, "../main", head.id(), head.id()),
                ErrorCode::InvalidRequest,
            ),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(request))
                    .await
                    .unwrap_err()
                    .code,
                error
            );
        }
        let mut stale_checkout = command(id, "merged", head.id(), head.id());
        if let Command::DeleteGitBranch {
            expected_branch, ..
        } = &mut stale_checkout
        {
            *expected_branch = None;
        }
        assert_eq!(
            client
                .execute(client.prepare(stale_checkout))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        // A pre-existing ref lock exposes libgit2's partial metadata failure:
        // tracking config is removed before ref deletion, whose result is unknown.
        let mut locks = repository.transaction().unwrap();
        locks.lock_ref("refs/heads/merged").unwrap();
        repository
            .config()
            .unwrap()
            .set_str("branch.merged.remote", "origin")
            .unwrap();
        let request = client.prepare(command(id, "merged", head.id(), head.id()));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(
            git2::Config::open(&repository.commondir().join("config"))
                .unwrap()
                .get_string("branch.merged.remote")
                .is_err()
        );
        drop(locks);
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(repository.find_branch("merged", BranchType::Local).is_ok());
        assert!(
            repository
                .find_branch("unmerged", BranchType::Local)
                .is_ok()
        );
        assert_eq!(repository.head().unwrap().target(), Some(head.id()));
    }
    // Detached HEAD still retains the merged commit and can delete an unused ref.
    repository.set_head_detached(head.id()).unwrap();
    let mut detached = command(id, "merged", head.id(), head.id());
    if let Command::DeleteGitBranch {
        expected_branch, ..
    } = &mut detached
    {
        *expected_branch = None;
    }
    remote.execute(remote.prepare(detached)).await.unwrap();
    assert!(repository.head_detached().unwrap());
    assert!(repository.find_branch("merged", BranchType::Local).is_err());
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn lost_result_is_not_replayed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    commit(&repository);
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("removed", &head, false).unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let request = client.prepare(command(id, "removed", head.id(), head.id()));
    assert_eq!(
        client.execute(request.clone()).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert!(
        repository
            .find_branch("removed", BranchType::Local)
            .is_err()
    );
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    node.shutdown().await.unwrap();
    repository.branch("removed", &head, false).unwrap();
    let node = Node::start(&profile).await.unwrap();
    assert_eq!(
        Client::new(node.local())
            .execute(request)
            .await
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    assert!(repository.find_branch("removed", BranchType::Local).is_ok());
    node.shutdown().await.unwrap();
}
