use super::*;

#[cfg(unix)]
#[tokio::test]
async fn requires_command_approval_before_execution() {
    for remote in [false, true] {
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Run the command after approval"}),
        )])
        .await;
        let child = Server::turn_tools(vec![(
            plugin_tool("commands", "run_command"),
            json!({"access":"unrestricted","command":"printf approved >> approved.txt"}),
        )])
        .await;
        let fixture = fixture::Fixture::start(remote, parent, child).await;
        let turn = fixture.push("Delegate the command").await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        assert_eq!(child.config.mode, WorkMode::Code);
        assert_eq!(child.config.permission, Permission::Ask);
        // Observe the same ordered subscription used by controller approval views.
        let approval = crate::approvals::rules::pending(&fixture.client, child.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(approval.session, child.id);
        assert!(!fixture.root().join("approved.txt").exists());
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: child.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let page = finished(&fixture.client, child.id, approval.turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.approvals[0].state, ApprovalState::Approved);
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("approved.txt")).unwrap(),
            "approved"
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);
        fixture.close().await;
    }
}

#[tokio::test]
async fn isolates_call_identity() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("source.txt"), "Read 中文 🙂").unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
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
        let mut config = session.config.clone();
        config
            .options
            .entry("delegation".into())
            .or_default()
            .insert("mode".into(), json!("multitask"));
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
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
            results(&client, &first_history).await
        );
        assert_eq!(
            std::fs::read_to_string(root.join("written.txt")).unwrap(),
            "Child 中文 🙂"
        );
        for child in &children {
            let page = history(&client, child.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            assert_eq!(
                results(&client, &page).await[0]["data"]["lines"][0]["text"],
                "Read 中文 🙂"
            );
            assert_eq!(page.approvals.len(), 1);
            assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part|
                matches!(part, Part::ToolCall { id: Some(id), name, .. } if id == "call-fixture-1" && name == &plugin_tool("files", "write_file"))));
        }
        assert_eq!(child.requests.lock().unwrap().len(), 6);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
