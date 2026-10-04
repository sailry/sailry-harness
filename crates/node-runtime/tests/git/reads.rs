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
    let node = Node::start(directory.path().join("node")).await.unwrap();
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
    let (local, remote) = tokio::join!(read(&local, worktree), read(&remote, worktree));
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
    for [status, branches, log] in [local, remote] {
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
