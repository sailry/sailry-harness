use super::*;

#[tokio::test]
async fn preserves_unknown_outcome() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::WriteFiles],
        )
        .await;
        let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER lose_action_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
        let request = write(&client, &context, "uncertain.txt");
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Admitted
        );
        assert_eq!(
            fs::read_to_string(root.join("uncertain.txt")).unwrap(),
            "完整结果 🙂"
        );
        fs::write(root.join("uncertain.txt"), "External replacement").unwrap();
        db.execute_batch("DROP TRIGGER lose_action_result").unwrap();
        drop(db);
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Unknown
        );
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            fs::read_to_string(root.join("uncertain.txt")).unwrap(),
            "External replacement"
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
