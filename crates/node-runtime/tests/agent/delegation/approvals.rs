use super::*;

#[tokio::test]
async fn isolates_call_identity() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("source.txt"), "Read 中文 🙂").unwrap();
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
            controller.handle().remote(address.clone())
        } else {
            node.local()
        });
        let parent = Server::parallel(vec![
            (
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role": "review", "task": "Read and write A"}),
            ),
            (
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role": "review", "task": "Read and write B"}),
            ),
        ])
        .await;
        let child = Server::tools(vec![
            (
                plugin_tool("files", "read_file"),
                json!({"path": "source.txt"}),
            ),
            (
                plugin_tool("files", "write_file"),
                json!({"path": "written.txt", "text": "Child 中文 🙂", "expected_revision": null}),
            ),
        ])
        .await;
        let (session, _, _) = setup(&client, &root, &parent, &child).await;
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 2,
                message: "Delegate both tasks".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let children = children(&client, session.id, 2).await;
        let (_, first) = crate::approvals::pending(&client, children[0].id).await;
        let (_, second) = crate::approvals::pending(&client, children[1].id).await;
        assert_ne!(first.id, second.id);
        assert_ne!(first.turn, second.turn);
        assert!(history(&client, session.id).await.approvals.is_empty());
        assert!(!root.join("written.txt").exists());
        drop(client);
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        assert_eq!(
            client
                .execute(client.prepare(Command::ResolveApproval {
                    session: children[0].id,
                    approval: second.id,
                    decision: Decision::Approve,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        for (child, approval, decision) in [
            (&children[0], &first, Decision::Approve),
            (&children[1], &second, Decision::Deny),
        ] {
            let request = client.prepare(Command::ResolveApproval {
                session: child.id,
                approval: approval.id,
                decision,
            });
            let result = client.execute(request.clone()).await.unwrap();
            assert_eq!(client.execute(request).await.unwrap(), result);
        }
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let first_history = history(&client, children[0].id).await;
        assert!(
            root.join("written.txt").exists(),
            "approved child results: {:?}",
            results(&first_history)
        );
        assert_eq!(
            std::fs::read_to_string(root.join("written.txt")).unwrap(),
            "Child 中文 🙂"
        );
        for child in &children {
            let page = history(&client, child.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            assert_eq!(results(&page)[0]["data"]["text"], "Read 中文 🙂");
            assert_eq!(page.approvals.len(), 1);
            assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part|
                matches!(part, Part::ToolCall { id: Some(id), name, .. } if id == "call-fixture-1" && name == &plugin_tool("files", "write_file"))));
        }
        assert_eq!(child.requests.lock().unwrap().len(), 6);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
