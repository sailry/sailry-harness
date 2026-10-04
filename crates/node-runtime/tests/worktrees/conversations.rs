use super::*;
use sailry_link::{Link, NetworkScope};
use std::path::Path;

#[tokio::test]
async fn creates_and_relocates() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = creation::init(&root);
    std::fs::create_dir(root.join("old")).unwrap();
    std::fs::write(root.join("old/removed"), "removed\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("old/removed")).unwrap();
    index.write().unwrap();
    let parent = repository.head().unwrap().peel_to_commit().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(
            Some("HEAD"),
            &author,
            &author,
            "Nested files",
            &tree,
            &[&parent],
        )
        .unwrap();
    std::fs::remove_dir_all(root.join("old")).unwrap();
    std::fs::create_dir_all(root.join("new/nested")).unwrap();
    std::fs::write(root.join("new/nested/build notes.md"), "added\n").unwrap();
    std::fs::write(root.join("file"), "staged").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("file")).unwrap();
    index.write().unwrap();
    std::fs::write(root.join("file"), "working").unwrap();
    std::fs::write(root.join("untracked"), b"\0binary\xff").unwrap();
    let original_index = std::fs::read(repository.path().join("index")).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let project = register(&local, &root).await;
    let source = snapshot(&local).await.worktrees[0].id;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (number, client) in [&local, &remote].into_iter().enumerate() {
        let Output::GitStatus(status) = client
            .execute(client.prepare(Command::InspectGit { worktree: source }))
            .await
            .unwrap()
        else {
            panic!("status expected")
        };
        let request = client.prepare(Command::CreateManagedWorktree {
            project: project.id,
            source,
            branch: format!("isolated-{number}"),
            expected_head: status.head.unwrap(),
            expected_index: status.index_revision.unwrap(),
            include_changes: true,
        });
        let Output::Worktree(tree) = client.execute(request.clone()).await.unwrap() else {
            panic!("worktree expected")
        };
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::Worktree(tree.clone())
        );
        assert_eq!(
            std::fs::read(Path::new(&tree.path).join("file")).unwrap(),
            b"working"
        );
        assert_eq!(
            std::fs::read(Path::new(&tree.path).join("untracked")).unwrap(),
            b"\0binary\xff"
        );
        assert!(!Path::new(&tree.path).join("old/removed").exists());
        assert_eq!(
            std::fs::read(Path::new(&tree.path).join("new/nested/build notes.md")).unwrap(),
            b"added\n"
        );
        let linked = git2::Repository::open(&tree.path).unwrap();
        let entry = linked
            .index()
            .unwrap()
            .get_path(Path::new("file"), 0)
            .unwrap();
        assert_eq!(linked.find_blob(entry.id).unwrap().content(), b"staged");
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: Some(source),
                config: Some(config()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let movement = client.prepare(Command::MoveConversation {
            session: session.id,
            worktree: tree.id,
            expected_revision: 1,
        });
        let Output::Session(moved) = client.execute(movement.clone()).await.unwrap() else {
            panic!("session expected")
        };
        assert_eq!(
            client.execute(movement).await.unwrap(),
            Output::Session(moved.clone())
        );
        assert_eq!(moved.id, session.id);
        assert_eq!(moved.worktree, tree.id);
        let Output::Session(fork) = client
            .execute(client.prepare(Command::ForkConversationAt {
                session: moved.id,
                worktree: source,
                expected_revision: moved.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_ne!(fork.id, moved.id);
        assert_eq!(fork.worktree, source);
        assert_eq!(
            snapshot(client)
                .await
                .sessions
                .iter()
                .find(|entry| entry.id == moved.id)
                .unwrap()
                .worktree,
            tree.id
        );
    }
    assert_eq!(std::fs::read(root.join("file")).unwrap(), b"working");
    let original = repository
        .index()
        .unwrap()
        .get_path(Path::new("file"), 0)
        .unwrap();
    assert_eq!(
        repository.find_blob(original.id).unwrap().content(),
        b"staged"
    );
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        original_index
    );
    assert!(!root.join("old/removed").exists());
    assert_eq!(
        std::fs::read(root.join("new/nested/build notes.md")).unwrap(),
        b"added\n"
    );
    let before = snapshot(&local).await;
    let profile = node.profile().to_owned();
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start(profile).await.unwrap();
    assert_eq!(
        snapshot(&Client::new(node.local())).await.sessions,
        before.sessions
    );
    node.shutdown().await.unwrap();
}
