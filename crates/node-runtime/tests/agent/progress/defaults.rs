use super::*;

#[tokio::test]
async fn adds_progress_to_existing_conversations() {
    for remote in [false, true] {
        let server = Server::scripted_tools(Vec::new()).await;
        let fixture = Fixture::packages(remote, &server, &["files"]).await;
        let Output::QueuedTurn(previous) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Inspect the task".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        assert!(
            !previous
                .plugins
                .iter()
                .any(|package| package.name == "progress")
        );
        let page = finished(&fixture.client, fixture.session.id, previous.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::ReadPlugin {
                name: "progress".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("progress plugin expected")
        };
        assert!(package.summary.enabled && package.issues.is_empty());
        assert_eq!(
            client
                .read_turn_capture(fixture.session.id, previous.id)
                .await
                .unwrap(),
            previous
        );
        server.replace_tools(vec![
            (
                "tool_search".into(),
                json!({"query":plugin_tool("progress", "update_plan"),"limit":1}),
            ),
            (plugin_tool("progress", "update_plan"), plan("completed")),
        ]);
        let Output::QueuedTurn(next) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Report task progress".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        assert!(next.plugins.contains(&package.summary.reference()));
        let page = finished(&client, fixture.session.id, next.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == next.id)
                .unwrap()
                .status,
            Status::Completed
        );
        let view = observe(&client, fixture.session.id).await;
        assert_eq!(view.calls.len(), 2);
        assert_eq!(view.calls[0].name, "tool_search");
        assert_eq!(
            serde_json::to_value(view.calls[1].progress.as_ref().unwrap()).unwrap(),
            plan("completed")
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        for (index, available) in [(0, false), (1, false), (2, true), (3, true)] {
            assert_eq!(
                requests[index]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == plugin_tool("progress", "update_plan")),
                available
            );
        }
        assert_eq!(
            client
                .read_turn_capture(fixture.session.id, previous.id)
                .await
                .unwrap(),
            previous
        );
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_disabled_and_removed_defaults() {
    for remote in [false, true] {
        for removed in [false, true] {
            let server = Server::tools(vec![
                (
                    "tool_search".into(),
                    json!({"query":plugin_tool("progress", "update_plan"),"limit":1}),
                ),
                (plugin_tool("progress", "update_plan"), plan("completed")),
            ])
            .await;
            let fixture = Fixture::new(remote, &server).await;
            assert_eq!(fixture.session.config.mode, WorkMode::Code);
            let Output::Plugin(package) = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadPlugin {
                    name: "progress".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("progress plugin expected")
            };
            assert!(package.summary.enabled && package.issues.is_empty());
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "Report task progress".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            assert!(turn.plugins.contains(&package.summary.reference()));
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            assert!(page.approvals.is_empty());
            let view = observe(&fixture.client, fixture.session.id).await;
            assert_eq!(view.calls.len(), 2);
            assert_eq!(view.calls[0].name, "tool_search");
            assert_eq!(
                serde_json::to_value(view.calls[1].progress.as_ref().unwrap()).unwrap(),
                plan("completed")
            );
            for (index, request) in server.requests.lock().unwrap().iter().enumerate() {
                assert_eq!(
                    request["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tool| tool["function"]["name"]
                            == plugin_tool("progress", "update_plan")),
                    index > 0
                );
            }
            let change = if removed {
                Command::RemovePlugin {
                    name: "progress".into(),
                    expected_revision: package.summary.revision,
                }
            } else {
                Command::SetPluginEnabled {
                    name: "progress".into(),
                    expected_revision: package.summary.revision,
                    enabled: false,
                }
            };
            fixture
                .client
                .execute(fixture.client.prepare(change))
                .await
                .unwrap();
            let profile = fixture.node.profile().to_owned();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start(profile).await.unwrap();
            let client = Client::new(if remote {
                fixture.controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let installed = client
                .execute(client.prepare(Command::ReadPlugin {
                    name: "progress".into(),
                }))
                .await;
            if removed {
                assert_eq!(installed.unwrap_err().code, ErrorCode::NotFound);
            } else {
                let Output::Plugin(package) = installed.unwrap() else {
                    panic!("disabled progress plugin expected")
                };
                assert!(!package.summary.enabled);
            }
            assert_eq!(
                client
                    .read_turn_capture(fixture.session.id, turn.id)
                    .await
                    .unwrap(),
                turn
            );
            assert_eq!(observe(&client, fixture.session.id).await.calls, view.calls);
            let Output::QueuedTurn(next) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "Report the result".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            assert!(
                !next
                    .plugins
                    .iter()
                    .any(|package| package.name == "progress")
            );
            let page = finished(&client, fixture.session.id, next.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == next.id)
                    .unwrap()
                    .status,
                Status::Completed
            );
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 4);
            assert!(
                !requests.last().unwrap()["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == plugin_tool("progress", "update_plan"))
            );
            assert_eq!(observe(&client, fixture.session.id).await.calls, view.calls);
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
