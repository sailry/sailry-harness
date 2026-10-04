use super::*;

#[tokio::test]
async fn retains_admitted_versions() {
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            "read_skill_resource".into(),
            json!({"skill": "example:analysis", "path": "references/guide.md"}),
        )])
        .await;
        let (directory, node, controller, client, session) = fixture(remote, &server).await;
        enabled_only(&client, &["example"]).await;
        let queue = || Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Read the installed skill".into(),
        };
        let Output::QueuedTurn(old) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        package(&directory.path().join("project"), "2.0.0");
        let updated = install(&client, session.worktree, 1).await;
        assert_ne!(updated.summary.digest, old.plugins[0].digest);
        let Output::QueuedTurn(new) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        assert_eq!(new.plugins, [updated.summary.reference()]);
        assert_eq!(old.revision, new.revision);
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: updated.summary.revision,
                enabled: false,
            },
        )
        .await;
        let Output::QueuedTurn(disabled) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        assert!(disabled.plugins.is_empty());
        assert_eq!(disabled.revision, session.revision);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        for (turn, version) in [(&old, "1.0.0"), (&new, "2.0.0")] {
            execute(&client, Command::StartQueuedTurn { turn: turn.id }).await;
            let page = finished(&client, session.id, turn.id).await;
            assert_eq!(
                results(&page).last().unwrap()["content"],
                format!("Guide {version} 中文 🙂")
            );
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Completed
            );
        }
        let Output::Session(branch) = execute(
            &client,
            Command::ForkConversation {
                session: session.id,
                expected_revision: session.revision,
                through: old.id,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        assert_eq!(branch.config, session.config);
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert!(snapshot.turns.contains(&old));
        assert!(snapshot.turns.contains(&new));
        assert!(snapshot.turns.contains(&disabled));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
