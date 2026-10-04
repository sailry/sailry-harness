use super::*;
use git2::{BranchType, Config, Oid};

fn command(worktree: WorktreeId, name: &str, new_name: &str, tip: Oid) -> Command {
    Command::RenameGitBranch {
        worktree,
        name: name.into(),
        new_name: new_name.into(),
        commit: tip.to_string(),
    }
}

#[tokio::test]
async fn local_and_remote_rename() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file"), "base").unwrap();
    commit(&repository);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    repository
        .reference("refs/remotes/origin/main", base.id(), false, "Fixture")
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
    for (sequence, client) in [&local, &remote].into_iter().enumerate() {
        let name = format!("feature/中文-{sequence}");
        repository.branch(&name, &base, false).unwrap();
        for (key, value) in [("remote", "origin"), ("merge", "refs/heads/main")] {
            Config::open(&repository.commondir().join("config"))
                .unwrap()
                .set_str(&format!("branch.{name}.{key}"), value)
                .unwrap();
        }
        let renamed = format!("renamed/中文-{sequence}");
        let request = client.prepare(command(id, &name, &renamed, base.id()));
        assert!(request.command.durable());
        let output = client.execute(request.clone()).await.unwrap();
        assert_eq!(
            output,
            Output::GitBranchRenamed(GitBranch {
                name: renamed.clone(),
                commit: base.id().to_string(),
                remote: false,
                current: false,
            })
        );
        assert!(repository.find_branch(&name, BranchType::Local).is_err());
        assert_eq!(
            repository
                .find_branch(&renamed, BranchType::Local)
                .unwrap()
                .get()
                .target(),
            Some(base.id())
        );
        let config = Config::open(&repository.commondir().join("config")).unwrap();
        assert!(config.get_string(&format!("branch.{name}.remote")).is_err());
        assert_eq!(
            config
                .get_string(&format!("branch.{renamed}.remote"))
                .unwrap(),
            "origin"
        );
        assert_eq!(
            config
                .get_string(&format!("branch.{renamed}.merge"))
                .unwrap(),
            "refs/heads/main"
        );
        assert!(
            repository
                .reflog(&format!("refs/heads/{renamed}"))
                .unwrap()
                .len()
                >= 2
        );
        repository.branch(&name, &base, false).unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        assert!(repository.find_branch(&name, BranchType::Local).is_ok());

        let current = format!("main-{sequence}");
        let output = client
            .execute(client.prepare(command(id, "main", &current, base.id())))
            .await
            .unwrap();
        assert!(matches!(output, Output::GitBranchRenamed(branch) if branch.current));
        assert_eq!(
            repository.head().unwrap().name().unwrap(),
            format!("refs/heads/{current}")
        );
        client
            .execute(client.prepare(command(id, &current, "main", base.id())))
            .await
            .unwrap();
    }
    assert_eq!(
        repository
            .find_reference("refs/remotes/origin/main")
            .unwrap()
            .target(),
        Some(base.id())
    );
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        index
    );
    assert_eq!(std::fs::read(root.join("file")).unwrap(), b"uncommitted");
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn worktree_heads_follow_rename() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file"), "base").unwrap();
    commit(&repository);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    let reference = repository
        .branch("linked", &base, false)
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
    let linked_repository = Repository::open(&linked).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
    let linked_id = worktree(&local, &linked).await;
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    std::fs::write(linked.join("file"), "linked draft").unwrap();
    let index = std::fs::read(linked_repository.path().join("index")).unwrap();
    for client in [&local, &remote] {
        let merge_head = linked_repository.path().join("MERGE_HEAD");
        std::fs::write(&merge_head, format!("{}\n", base.id())).unwrap();
        assert_eq!(
            client
                .execute(client.prepare(command(id, "linked", "renamed", base.id())))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            linked_repository.head().unwrap().name().unwrap(),
            "refs/heads/linked"
        );
        std::fs::remove_file(merge_head).unwrap();
        client
            .execute(client.prepare(command(id, "linked", "renamed", base.id())))
            .await
            .unwrap();
        assert_eq!(
            linked_repository.head().unwrap().name().unwrap(),
            "refs/heads/renamed"
        );
        let output = client
            .execute(client.prepare(command(linked_id, "renamed", "linked", base.id())))
            .await
            .unwrap();
        assert!(matches!(output, Output::GitBranchRenamed(branch) if branch.current));
        client
            .execute(client.prepare(command(linked_id, "main", "main-renamed", base.id())))
            .await
            .unwrap();
        assert_eq!(
            repository.head().unwrap().name().unwrap(),
            "refs/heads/main-renamed"
        );
        client
            .execute(client.prepare(command(linked_id, "main-renamed", "main", base.id())))
            .await
            .unwrap();
    }
    assert_eq!(
        std::fs::read(linked_repository.path().join("index")).unwrap(),
        index
    );
    assert_eq!(std::fs::read(linked.join("file")).unwrap(), b"linked draft");
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn conflicts_and_partial_failure() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    commit(&repository);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("feature", &base, false).unwrap();
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
    for (sequence, client) in [&local, &remote].into_iter().enumerate() {
        for (command, error) in [
            (
                command(id, "main", "feature", base.id()),
                ErrorCode::Conflict,
            ),
            (
                command(id, "missing", "new", base.id()),
                ErrorCode::NotFound,
            ),
            (
                command(id, "main", "new", Oid::ZERO_SHA1),
                ErrorCode::RevisionConflict,
            ),
            (
                command(id, "main", "../new", base.id()),
                ErrorCode::InvalidRequest,
            ),
            (
                command(id, "main", "HEAD", base.id()),
                ErrorCode::InvalidRequest,
            ),
            (
                command(id, "../main", "new", base.id()),
                ErrorCode::InvalidRequest,
            ),
            (
                command(WorktreeId::new(), "main", "new", base.id()),
                ErrorCode::NotFound,
            ),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                error
            );
        }
        let noop = client
            .execute(client.prepare(command(id, "main", "main", base.id())))
            .await
            .unwrap();
        assert!(
            matches!(noop, Output::GitBranchRenamed(branch) if branch.name == "main" && branch.current)
        );

        let name = format!("partial-{sequence}");
        repository.branch(&name, &base, false).unwrap();
        Config::open(&repository.commondir().join("config"))
            .unwrap()
            .set_str(&format!("branch.{name}.remote"), "origin")
            .unwrap();
        let lock = repository.commondir().join("config.lock");
        std::fs::write(&lock, "fixture lock").unwrap();
        let renamed = format!("renamed-{sequence}");
        let request = client.prepare(command(id, &name, &renamed, base.id()));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(repository.find_branch(&name, BranchType::Local).is_err());
        assert!(repository.find_branch(&renamed, BranchType::Local).is_ok());
        assert_eq!(
            Config::open(&repository.commondir().join("config"))
                .unwrap()
                .get_string(&format!("branch.{name}.remote"))
                .unwrap(),
            "origin"
        );
        std::fs::remove_file(lock).unwrap();
        repository.branch(&name, &base, false).unwrap();
        repository
            .find_branch(&renamed, BranchType::Local)
            .unwrap()
            .delete()
            .unwrap();
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(repository.find_branch(&name, BranchType::Local).is_ok());
        assert!(repository.find_branch(&renamed, BranchType::Local).is_err());
    }
    assert_eq!(
        repository.head().unwrap().name().unwrap(),
        "refs/heads/main"
    );
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
    repository.branch("feature", &head, false).unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let request = client.prepare(command(id, "feature", "renamed", head.id()));
    assert_eq!(
        client.execute(request.clone()).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert!(
        repository
            .find_branch("feature", BranchType::Local)
            .is_err()
    );
    assert!(repository.find_branch("renamed", BranchType::Local).is_ok());
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    node.shutdown().await.unwrap();
    repository.branch("feature", &head, false).unwrap();
    repository
        .find_branch("renamed", BranchType::Local)
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
    assert!(repository.find_branch("feature", BranchType::Local).is_ok());
    assert!(
        repository
            .find_branch("renamed", BranchType::Local)
            .is_err()
    );
    node.shutdown().await.unwrap();
}
