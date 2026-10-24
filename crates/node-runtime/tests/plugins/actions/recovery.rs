use super::*;

#[tokio::test]
async fn preserves_unknown_outcome() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let root = directory.path().join("source");
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::WriteFiles],
        )
        .await;

        { let sql = ("CREATE TRIGGER lose_action_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").to_owned();
node.with_storage(move |db| Box::pin(async move { db.execute_batch(sql).await.map_err(crate::plugins::database_fixture::fault) })).await }.unwrap();
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
        {
            let sql = ("DROP TRIGGER lose_action_result").to_owned();
            node.with_storage(move |db| {
                Box::pin(async move {
                    db.execute_batch(sql)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)
                })
            })
            .await
        }
        .unwrap();

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
        let node = Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
        .await
        .unwrap();
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
