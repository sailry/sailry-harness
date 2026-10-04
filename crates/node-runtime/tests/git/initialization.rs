use super::*;

#[tokio::test]
async fn initializes_once_without_replacing_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
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
