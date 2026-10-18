use super::*;

#[tokio::test]
async fn reads_exact_source_pages_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    let original = "first\r\nold\nlast";
    let staged = "first\r\nindex\nlast-index";
    std::fs::write(root.join("file.txt"), original).unwrap();
    commit(&repository);
    std::fs::write(root.join("file.txt"), staged).unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("file.txt")).unwrap();
    index.write().unwrap();
    std::fs::write(root.join("file.txt"), "uncommitted source\n").unwrap();
    let before = std::fs::read(repository.path().join("index")).unwrap();
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let local = Client::new(node.local());
    let worktree = worktree(&local, &root).await;
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let peer = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(peer));
    for client in [&local, &remote] {
        let Output::GitChanges(scope) = client
            .execute(client.prepare(Command::ListGitChanges {
                worktree,
                target: Some(GitChangeTarget::Worktree {
                    scope: GitDiffScope::Staged,
                }),
                cursor: None,
                limit: 100,
            }))
            .await
            .unwrap()
        else {
            panic!("inventory expected")
        };
        let metadata = scope
            .entries
            .iter()
            .find(|entry| entry.path == "file.txt")
            .unwrap()
            .metadata_revision
            .clone();
        let Output::GitChange(diff) = client
            .execute(client.prepare(Command::ReadGitChange {
                worktree,
                snapshot: scope.snapshot.clone(),
                path: "file.txt".into(),
                expected_metadata_revision: metadata.clone(),
                expected_revision: None,
                offset: 0,
                limit: 100,
            }))
            .await
            .unwrap()
        else {
            panic!("change expected")
        };
        for (side, expected) in [
            (GitContextSide::Old, original),
            (GitContextSide::New, staged),
        ] {
            let mut text = String::new();
            let mut offset = 0;
            loop {
                let request = client.prepare(Command::ReadGitContext {
                    worktree,
                    snapshot: scope.snapshot.clone(),
                    path: "file.txt".into(),
                    expected_metadata_revision: metadata.clone(),
                    expected_revision: diff.revision.clone(),
                    side,
                    offset,
                    limit: 1,
                });
                let admitted = client.dispatch(request).await.unwrap();
                assert!(!admitted.receipt.durable);
                let Output::GitContext(page) = admitted.completion.await.unwrap().unwrap() else {
                    panic!("context expected")
                };
                assert_eq!(page.snapshot, scope.snapshot);
                assert_eq!(page.revision, diff.revision);
                assert!(page.skip_reason.is_none());
                text.push_str(&page.text);
                let Some(next) = page.next else { break };
                assert_eq!(next, offset + 1);
                offset = next;
            }
            assert_eq!(text, expected);
        }
    }
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "uncommitted source\n"
    );
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}
