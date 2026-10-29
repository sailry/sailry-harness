use super::*;

#[tokio::test]
async fn uses_captured_class_declarations() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![(
            plugin_tool(PACKAGE, "write"),
            draft(None, None, "Captured"),
        )])
        .await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        let original = install(&fixture, 0, "<html><body>Original renderer</body></html>").await;
        let Output::QueuedTurn(queued) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Create the requested document".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        assert!(queued.plugins.contains(&original.summary.reference()));
        let updated = install_at(
            &fixture.client,
            fixture.session.worktree,
            &fixture.root,
            original.summary.revision,
            "<html><body>Updated renderer</body></html>",
            &["chart", "replacement-accent"],
        )
        .await;
        assert_ne!(updated.summary.digest, original.summary.digest);
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StartQueuedTurn { turn: queued.id }),
            )
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let first = fixture
            .client
            .list_artifacts(fixture.session.id, None, 1)
            .await
            .unwrap()
            .items
            .remove(0);
        let captured = fixture.client.read_artifact(first.id, None).await.unwrap();
        assert_eq!(captured.artifact.package, original.summary.reference());
        assert!(captured.html.contains("class=\"fixture-accent\""));
        assert!(!captured.html.contains("lead"));
        assert_eq!(
            captured.view.as_ref().unwrap().template,
            "<html><body>Original renderer</body></html>"
        );

        let turn = submit(&fixture, "Create another requested document").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let second = fixture
            .client
            .list_artifacts(fixture.session.id, None, 2)
            .await
            .unwrap()
            .items
            .into_iter()
            .find(|document| document.turn == turn)
            .unwrap();
        let current = fixture.client.read_artifact(second.id, None).await.unwrap();
        assert_eq!(current.artifact.package, updated.summary.reference());
        assert!(!current.html.contains("fixture-accent"));
        assert!(!current.html.contains("lead"));
        assert!(current.html.contains("class=\"chart\""));
        assert_eq!(
            current.view.as_ref().unwrap().template,
            "<html><body>Updated renderer</body></html>"
        );
        assert_eq!(
            fixture.client.read_artifact(first.id, None).await.unwrap(),
            captured
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_history_after_brief_removal() {
    for remote in [false, true] {
        let write = plugin_tool("brief", "write_brief");
        let server = Server::scripted_tools(vec![
            ("tool_search".into(), json!({"query":write,"limit":1})),
            (write.clone(), draft(None, None, "Historical document")),
        ])
        .await;
        let fixture = process::Fixture::packages(remote, &server, &["brief"]).await;
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPlugin {
                name: "brief".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("brief package expected")
        };
        assert!(package.summary.enabled);
        let instructions = package.extension.unwrap().instructions.unwrap();
        let turn = submit(&fixture, "Create the requested HTML document").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let document = fixture
            .client
            .list_artifacts(fixture.session.id, None, 1)
            .await
            .unwrap()
            .items
            .remove(0);
        let captured = fixture
            .client
            .read_artifact(document.id, Some(1))
            .await
            .unwrap();
        assert_eq!(captured.artifact.package, package.summary.reference());
        let renderer = captured.view.as_ref().unwrap();
        assert!(!renderer.template.is_empty());
        assert!(!renderer.styles.is_empty());
        assert!(!renderer.scripts.is_empty());
        {
            let requests = server.requests.lock().unwrap();
            let request = &requests[0];
            assert!(!system_prompt(request).contains(&instructions));
            assert!(has_tool(request, "tool_search"));
            assert!(!has_tool(request, "write_brief"));
            assert!(has_tool(&requests[1], "write_brief"));
            for name in ["read_brief", "list_briefs"] {
                assert!(!has_tool(&requests[1], name));
            }
            for (index, request) in requests.iter().enumerate() {
                assert_eq!(
                    node_context(request, "Work mode")
                        .matches(&instructions)
                        .count(),
                    usize::from(index > 0),
                );
                assert!(!turn_context(request).contains(&instructions));
                assert!(
                    request["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|message| {
                            message["role"] != "user"
                                || !message_text(&message["content"]).contains(&instructions)
                        })
                );
            }
        }

        fixture
            .client
            .execute(fixture.client.prepare(Command::RemovePlugin {
                name: "brief".into(),
                expected_revision: package.summary.revision,
            }))
            .await
            .unwrap();
        server.replace_tools(Vec::new());
        let first_request = server.requests.lock().unwrap().len();
        let turn = submit(&fixture, "Continue the conversation").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        {
            let requests = server.requests.lock().unwrap();
            assert!(requests.len() > first_request);
            for request in requests.iter().skip(first_request) {
                let current = node_context(request, "Work mode");
                assert!(!current.contains(&instructions));
                assert!(!current.contains("offer an optional HTML brief"));
                assert!(!system_prompt(request).contains(&instructions));
                assert!(!turn_context(request).contains(&instructions));
                for name in ["write_brief", "read_brief", "list_briefs"] {
                    assert!(!has_tool(request, name));
                }
            }
        }
        assert_eq!(
            fixture
                .client
                .read_artifact(document.id, Some(1))
                .await
                .unwrap(),
            captured
        );
        assert_eq!(cards(&fixture.client, &page).await, [document]);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

fn system_prompt(request: &Value) -> String {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| matches!(message["role"].as_str(), Some("system" | "developer")))
        .map(|message| message_text(&message["content"]))
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_tool(request: &Value, name: &str) -> bool {
    request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool.pointer("/function/name").and_then(Value::as_str) == Some(name))
}
