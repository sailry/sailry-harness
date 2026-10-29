use super::*;

#[tokio::test]
async fn retains_admitted_versions() {
    for remote in [false, true] {
        let gate = Arc::new(tokio::sync::Notify::new());
        let server = Server::held_turn_completion(
            vec![(
                "read_skill_resource".into(),
                json!({"skill": "example:analysis", "path": "references/guide.md"}),
            )],
            gate.clone(),
        )
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
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let queue = history(&client, session.id).await.queue;
        assert!(queue.paused);
        execute(
            &client,
            Command::SetQueuePaused {
                session: session.id,
                expected_revision: queue.revision,
                paused: false,
            },
        )
        .await;
        for (index, (turn, version)) in [(&old, "1.0.0"), (&new, "2.0.0")].into_iter().enumerate() {
            execute(&client, Command::StartQueuedTurn { turn: turn.id }).await;
            server.wait_count((index + 1) * 2).await;
            let page = history(&client, session.id).await;
            assert_eq!(
                results(&client, &page).await.last().unwrap()["content"],
                format!("Guide {version} 中文 🙂")
            );
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Running
            );
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), (index + 1) * 2);
            let request = requests.last().unwrap();
            let content = format!("Guide {version} 中文 🙂");
            input::assert_content(request, &[&content]);
            let receipt = input::receipt(request, "call-fixture-0");
            let original = input::original(&client, turn.id, &receipt).await;
            assert_eq!(original["content"], content);
            assert_eq!(input::owner(request, "references/guide.md"), original);
            assert_eq!(receipt["directory"], original["directory"]);
            if index == 1 {
                let previous = request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|message| message["role"] == "tool")
                    .find_map(|message| {
                        let result: Value =
                            serde_json::from_str(message["content"].as_str()?).ok()?;
                        (result["content"] == "Guide 1.0.0 中文 🙂").then_some(result)
                    })
                    .unwrap();
                assert_ne!(previous["directory"], receipt["directory"]);
                assert!(previous.get("output").is_none());
            }
            gate.notify_one();
            let page = finished(&client, session.id, turn.id).await;
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
        for turn in [&old, &new, &disabled] {
            assert!(snapshot.turns.contains(&TurnSummary::from(turn)));
            assert_eq!(
                client.read_turn_capture(session.id, turn.id).await.unwrap(),
                *turn
            );
        }
        assert_eq!(
            client.read_turn_capture(branch.id, old.id).await.unwrap(),
            old
        );
        assert!(client.read_turn_capture(branch.id, new.id).await.is_err());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
