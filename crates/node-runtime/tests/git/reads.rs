use super::*;

#[tokio::test]
async fn serves_concurrent_controllers() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("note.txt"), "original").unwrap();
    commit(&repository);
    std::fs::write(root.join("note.txt"), "changed").unwrap();
    let index = std::fs::read(repository.path().join("index")).unwrap();
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let reads = std::sync::Arc::new(sailry_client::git::Reads::default());
    let local = Client::new(node.local()).with_git_reads(reads.clone());
    let worktree = worktree(&local, &root).await;
    let controller = Link::controller(directory.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let peer = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(peer)).with_git_reads(reads);
    let other_root = directory.path().join("other-repository");
    let other_repository = Repository::init(&other_root).unwrap();
    other_repository.set_head("refs/heads/main").unwrap();
    std::fs::write(other_root.join("note.txt"), "original").unwrap();
    commit(&other_repository);
    std::fs::write(other_root.join("note.txt"), "changed").unwrap();
    let other_index = std::fs::read(other_repository.path().join("index")).unwrap();
    let other_worktree = super::worktree(&local, &other_root).await;
    let (local, remote, other_local, other_remote) = tokio::join!(
        read(&local, worktree),
        read(&remote, worktree),
        read(&local, other_worktree),
        read(&remote, other_worktree)
    );
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
    for [status, branches, log] in [local, remote, other_local, other_remote] {
        let Output::GitStatus(status) = status.unwrap() else {
            panic!("status expected")
        };
        let Output::GitBranches(branches) = branches.unwrap() else {
            panic!("branches expected")
        };
        let Output::GitLog(log) = log.unwrap() else {
            panic!("history expected")
        };
        assert_eq!(status.entries.len(), 1);
        assert_eq!(branches.current.as_deref(), Some("main"));
        assert_eq!(log.entries.len(), 1);
    }
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        index
    );
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "changed"
    );
    assert_eq!(
        std::fs::read(other_repository.path().join("index")).unwrap(),
        other_index
    );
    assert_eq!(
        std::fs::read_to_string(other_root.join("note.txt")).unwrap(),
        "changed"
    );
}

async fn read(client: &Client, worktree: WorktreeId) -> [Result<Output, Fault>; 3] {
    let (status, branches, log) = tokio::join!(
        client.execute(client.prepare(Command::InspectGit { worktree })),
        client.execute(client.prepare(Command::ListGitBranches { worktree })),
        client.execute(client.prepare(Command::ReadGitLog {
            worktree,
            limit: 10,
            cursor: None
        })),
    );
    [status, branches, log]
}

#[tokio::test]
async fn skips_ignored_trees_without_losing_tracked_changes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::create_dir(root.join("generated")).unwrap();
    std::fs::write(root.join("generated/tracked.txt"), "original\n").unwrap();
    std::fs::write(root.join("removed.txt"), "removed\n").unwrap();
    commit(&repository);
    std::fs::write(root.join(".gitignore"), "generated/\n").unwrap();
    std::fs::write(root.join("generated/tracked.txt"), "changed\n").unwrap();
    std::fs::remove_file(root.join("removed.txt")).unwrap();
    for index in 0..20_100 {
        std::fs::write(root.join(format!("generated/{index}.txt")), "ignored").unwrap();
    }
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let local = Client::new(node.local());
    let worktree = worktree(&local, &root).await;
    let controller = Link::controller(directory.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let peer = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(peer));
    for client in [&local, &remote] {
        let status = status(client, worktree).await;
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert!(!status.truncated);
        assert_eq!(status.omitted_paths, 0);
        assert_eq!(status.entries.len(), 3);
        assert!(
            status
                .entries
                .iter()
                .any(|entry| entry.path == "generated/tracked.txt"
                    && entry.unstaged == Some(GitChangeKind::Modified))
        );
        assert!(
            status
                .entries
                .iter()
                .any(|entry| entry.path == "removed.txt"
                    && entry.unstaged == Some(GitChangeKind::Deleted))
        );
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}
