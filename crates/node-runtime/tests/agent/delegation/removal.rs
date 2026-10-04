use super::*;

#[tokio::test]
async fn preserves_active_tasks_and_forks() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
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
        let parent = Server::tools(vec![(
            crate::agent_support::plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review", "task":"Wait for cancellation"}),
        )])
        .await;
        let child = Server::start(true).await;
        let (session, _, _) = setup(&client, &root, &parent, &child).await;
        let remove = Command::RemoveSession {
            session: session.id,
            expected_revision: session.revision,
        };
        let Output::QueuedTurn(queued) = client
            .execute(client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Wait in queue".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        assert_eq!(
            client
                .execute(client.prepare(remove.clone()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        client
            .execute(client.prepare(Command::StopTurn { turn: queued.id }))
            .await
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Delegate the task".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        child.wait_count(1).await;
        let children = children(&client, session.id, 1).await;
        assert_eq!(
            client
                .execute(client.prepare(remove.clone()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        assert!(
            client
                .execute(client.prepare(Command::RemoveSession {
                    session: children[0].id,
                    expected_revision: children[0].revision
                }))
                .await
                .is_err()
        );
        client
            .execute(client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        finished(&client, session.id, turn.id).await;
        let child_page = history(&client, children[0].id).await;
        finished(&client, children[0].id, child_page.runs[0].turn).await;
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
        let inherited = history(&client, branch.id).await;
        let Output::SessionsRemoved(ids) = client.execute(client.prepare(remove)).await.unwrap()
        else {
            panic!("removed sessions expected")
        };
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&session.id) && ids.contains(&children[0].id));
        for id in ids {
            assert_eq!(
                client
                    .read_conversation(id, None, 10)
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        }
        assert_eq!(history(&client, branch.id).await, inherited);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        assert_eq!(
            history(&Client::new(node.local()), branch.id).await,
            inherited
        );
        assert_eq!(child.requests.lock().unwrap().len(), 1);
        node.shutdown().await.unwrap();
    }
}
