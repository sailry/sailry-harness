use super::*;

#[tokio::test]
async fn persists_switches_and_allows_removal() {
    for remote in [false, true] {
        let (directory, node, controller, client, _worktree) = fixture(remote).await;
        let name = "git";
        let initial = info(execute(&client, Command::ReadPlugin { name: name.into() }).await);
        assert!(initial.summary.enabled);
        assert_eq!(initial.summary.revision, 1);
        let request = client.prepare(Command::SetPluginEnabled {
            name: name.into(),
            expected_revision: initial.summary.revision,
            enabled: false,
        });
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        let disabled = info(pending.completion.await.unwrap().unwrap());
        assert!(!disabled.summary.enabled);
        assert_eq!(disabled.summary.revision, 2);
        assert_eq!(info(client.execute(request).await.unwrap()), disabled);
        assert_eq!(
            client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: 0,
                    enabled: true,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(
            snapshot.plugins.iter().find(|entry| entry.name == name),
            Some(&disabled.summary)
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let restarted = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(restarted.local());
        assert_eq!(
            info(execute(&client, Command::ReadPlugin { name: name.into() }).await),
            disabled
        );
        let enabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: name.into(),
                    expected_revision: disabled.summary.revision,
                    enabled: true,
                },
            )
            .await,
        );
        assert!(enabled.summary.enabled);
        assert_eq!(enabled.summary.revision, 3);
        execute(
            &client,
            Command::RemovePlugin {
                name: name.into(),
                expected_revision: enabled.summary.revision,
            },
        )
        .await;
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        drop(client);
        restarted.shutdown().await.unwrap();
    }
}
