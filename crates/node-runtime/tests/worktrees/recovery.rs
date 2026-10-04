use super::{
    creation::init,
    discovery::{adopt, list},
    *,
};
use sailry_link::{Link, NetworkScope};

#[tokio::test]
async fn preserves_lost_registration() {
    for remote in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repository");
        let repository = init(&root);
        let linked = temp.path().join("linked");
        repository.worktree("linked", &linked, None).unwrap();
        let profile = temp.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let project = register(&client, &root).await;
        let before = snapshot(&client).await;
        let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
        let request = client.prepare(Command::RegisterWorktree {
            project: project.id,
            path: linked.to_str().unwrap().into(),
        });
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(snapshot(&client).await, before);
        db.execute_batch("DROP TRIGGER lose_result").unwrap();
        drop(db);
        node.shutdown().await.unwrap();
        std::fs::write(linked.join("file"), "preserved edit").unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(snapshot(&client).await, before);
        assert_eq!(list(&client, before.worktrees[0].id).await.entries.len(), 2);
        let recovered = adopt(&client, project.id, &linked).await;
        assert_eq!(adopt(&client, project.id, &linked).await, recovered);
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        let registered = snapshot(&client).await;
        assert_eq!(registered.worktrees.len(), 2);
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(snapshot(&client).await, registered);
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read(linked.join("file")).unwrap(),
            b"preserved edit"
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn bounded_listing_keeps_contexts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = init(&root);
    for index in 0..MAX_GIT_WORKTREES {
        let name = format!("linked-{index:03}");
        repository
            .worktree(&name, &temp.path().join(&name), None)
            .unwrap();
    }
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    register(&client, &root).await;
    let before = snapshot(&client).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&client, &remote] {
        let found = list(client, before.worktrees[0].id).await;
        assert_eq!(found.entries.len(), MAX_GIT_WORKTREES);
        assert!(found.truncated);
        assert_eq!(found.omitted_paths, 0);
        assert!(found.entries[0].main);
        assert!(found.entries.iter().all(|entry| entry.available));
        assert!(
            serde_json::to_vec(&Output::GitWorktrees(found))
                .unwrap()
                .len()
                < MAX_FRAME_BYTES
        );
        assert_eq!(snapshot(client).await, before);
    }
    assert_eq!(repository.worktrees().unwrap().len(), MAX_GIT_WORKTREES);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
