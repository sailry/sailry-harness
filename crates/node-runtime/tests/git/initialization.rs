use super::*;

#[tokio::test]
async fn initializes_once_without_replacing_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let node = Node::start_empty(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (position, client) in [&local, &remote].into_iter().enumerate() {
        let root = temp.path().join(format!("directory-{position}"));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("notes.txt"), "Keep this draft\n").unwrap();
        let worktree = worktree(client, &root).await;
        assert_eq!(
            status(client, worktree).await.kind,
            RepositoryKind::Directory
        );
        let request = client.prepare(Command::RunGitAction {
            worktree,
            action: GitAction::Initialize,
            expected_index: String::new(),
            expected_head: None,
            expected_branch: None,
        });
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        assert_eq!(
            admission.completion.await.unwrap().unwrap(),
            Output::GitActionCompleted
        );
        assert_eq!(
            client.execute(request.clone()).await.unwrap(),
            Output::GitActionCompleted
        );
        assert_eq!(status(client, worktree).await.kind, RepositoryKind::Unborn);
        let metadata = std::fs::read(root.join(".git/HEAD")).unwrap();
        let error = client
            .execute(client.prepare(request.command))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        assert_eq!(std::fs::read(root.join(".git/HEAD")).unwrap(), metadata);
        assert_eq!(
            std::fs::read_to_string(root.join("notes.txt")).unwrap(),
            "Keep this draft\n"
        );
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}

#[tokio::test]
async fn lists_child_repositories() {
    let temp = tempfile::tempdir().unwrap();
    let node = Node::start_empty(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (position, client) in [&local, &remote].into_iter().enumerate() {
        let root = temp.path().join(format!("workspace-{position}"));
        std::fs::create_dir(&root).unwrap();
        Repository::init(root.join("web")).unwrap();
        std::fs::create_dir(root.join("api")).unwrap();
        std::fs::write(root.join("api/.git"), "gitdir: ../web/.git\n").unwrap();
        Repository::init(root.join(".hidden")).unwrap();
        std::fs::create_dir(root.join("notes")).unwrap();
        std::fs::write(root.join("README.md"), "Workspace\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("web"), root.join("linked")).unwrap();
        let worktree = worktree(client, &root).await;
        let listed = status(client, worktree).await;
        assert_eq!(listed.kind, RepositoryKind::Directory);
        assert_eq!(listed.repositories, ["api", "web"]);
        let request = client.prepare(Command::RunGitAction {
            worktree,
            action: GitAction::Initialize,
            expected_index: String::new(),
            expected_head: None,
            expected_branch: None,
        });
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::GitActionCompleted
        );
        let initialized = status(client, worktree).await;
        assert_eq!(initialized.kind, RepositoryKind::Unborn);
        assert!(initialized.repositories.is_empty());
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}
