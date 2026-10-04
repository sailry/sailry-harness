use super::*;

#[tokio::test]
async fn freezes_config_and_history() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
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
        let parent = Server::parallel(vec![
            (
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role": "review", "task": "Inspect 中文 A"}),
            ),
            (
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role": "review", "task": "Inspect 中文 B"}),
            ),
        ])
        .await;
        // Neither model request completes until both child Runners have reached this endpoint.
        let child = Server::gate(2).await;
        let (session, mut provider, mut role) = setup(&client, &root, &parent, &child).await;
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = sailry_client::Projection::new(node.id(), 1);
        projection.apply(1, updates.next().await.unwrap()).unwrap();
        let request = client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: 2,
            message: "Delegate both independent tasks".into(),
        });
        let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
            panic!("turn expected")
        };
        role.instructions = "Changed catalog instructions".into();
        role.name = "Changed catalog name".into();
        client
            .execute(client.prepare(Command::PutRole {
                role: role.clone(),
                expected_revision: role.revision,
            }))
            .await
            .unwrap();
        provider.endpoint = "http://127.0.0.1:1/v1".into();
        client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: provider.revision,
            }))
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::StartQueuedTurn { turn: turn.id }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        let output = results(&page);
        assert_eq!(output.len(), 2, "{output:?}");
        assert_eq!(page.children.len(), 2);
        assert!(
            page.children
                .iter()
                .all(|child| child.name.as_deref() == Some("Review"))
        );
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            while projection.snapshot().unwrap().cursor < snapshot.cursor {
                projection.apply(1, updates.next().await.unwrap()).unwrap();
            }
        })
        .await
        .unwrap();
        assert_eq!(projection.snapshot().unwrap().sessions, snapshot.sessions);
        assert_eq!(projection.snapshot().unwrap().turns, snapshot.turns);
        drop(updates);
        let requests = child.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2, "{output:?}");
        for request in requests {
            assert_eq!(request["model"], "fixture-child");
            assert_eq!(request["reasoning_effort"], "low");
            let text = request["messages"].to_string();
            assert!(text.contains("Frozen review instructions"));
            assert!(!text.contains("Changed catalog"));
            assert!(!text.contains("Delegate both independent tasks"));
            assert_ne!(
                text.contains("Inspect 中文 A"),
                text.contains("Inspect 中文 B")
            );
            assert!(
                request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|tool| tool["function"]["name"]
                        != crate::agent_support::plugin_tool("delegation", "spawn_agent"))
            );
        }
        let mut histories = Vec::new();
        for result in &output {
            let call_id = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .find_map(|part| match part {
                    Part::ToolResult {
                        id, result: value, ..
                    } if value == result => id.as_ref(),
                    _ => None,
                })
                .unwrap();
            let (entry, index) = page.entries.iter().find_map(|entry| {
                entry.parts.iter()
                    .position(|part| matches!(part, Part::ToolCall { id: Some(id), .. } if id == call_id))
                    .map(|index| (entry, index))
            }).unwrap();
            assert_eq!(result["status"], "completed", "{result}");
            assert_eq!(result["response"], "answer-fixture-child");
            let id: SessionId = result["session"].as_str().unwrap().parse().unwrap();
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            let child = snapshot
                .sessions
                .iter()
                .find(|session| session.id == id)
                .unwrap();
            let origin = child.delegation.as_ref().unwrap();
            assert_eq!(origin.session, session.id);
            assert_eq!(origin.turn, turn.id);
            assert_eq!(origin.entry, entry.id);
            assert_eq!(origin.index, index);
            assert_eq!(origin.role, Some(role.id));
            assert_eq!(child.worktree, session.worktree);
            assert_eq!(child.config.permission, session.config.permission);
            for command in [
                Command::SubmitTurn {
                    session: id,
                    expected_revision: 1,
                    message: "Cannot resume child".into(),
                },
                Command::SetSessionRoles {
                    session: id,
                    expected_revision: 1,
                    roles: vec![],
                },
                Command::SetSessionConfig {
                    session: id,
                    expected_revision: 1,
                    config: child.config.clone(),
                },
            ] {
                assert_eq!(
                    client
                        .execute(client.prepare(command))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::PermissionDenied
                );
            }
            let history = history(&client, id).await;
            assert_eq!(history.runs.len(), 1);
            assert_eq!(history.runs[0].status, Status::Completed);
            let summary = page
                .children
                .iter()
                .find(|child| child.run.session == id)
                .unwrap();
            assert_eq!(summary.run, history.runs[0]);
            assert_eq!(&summary.origin, origin.as_ref());
            histories.push((id, history));
        }
        let Output::Session(branch) = client
            .execute(client.prepare(Command::ForkConversation {
                session: session.id,
                through: turn.id,
                expected_revision: session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        let inherited = client
            .read_conversation(branch.id, None, 20)
            .await
            .unwrap()
            .page;
        assert_eq!(inherited.children, page.children);
        assert_eq!(inherited.runs[0].origin, Some(session.id));
        let Output::Rewound(rewind) = client
            .execute(client.prepare(Command::RewindConversation {
                session: branch.id,
                through: None,
                expected_head: turn.id,
                expected_revision: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("rewind expected")
        };
        let backup = client
            .read_conversation(rewind.backup.id, None, 20)
            .await
            .unwrap()
            .page;
        assert_eq!(backup.children, page.children);
        let empty = client
            .read_conversation(branch.id, None, 20)
            .await
            .unwrap()
            .page;
        assert!(empty.children.is_empty());
        histories.push((branch.id, empty));
        histories.push((rewind.backup.id, backup));
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(parent.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(history(&client, session.id).await, page);
        for (id, page) in histories {
            assert_eq!(history(&client, id).await, page);
        }
        assert_eq!(child.requests.lock().unwrap().len(), 2);
        assert_eq!(parent.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
    }
}
