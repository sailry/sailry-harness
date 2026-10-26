use super::*;

fn submit(fixture: &Fixture, text: &str) -> Request {
    fixture.client.prepare(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        message: text.to_owned().into(),
    })
}

async fn seed(fixture: &Fixture, id: &str, state: &str) {
    let Output::Plugin(package) = fixture
        .client
        .execute(fixture.client.prepare(Command::ReadPlugin {
            name: "goals".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("package expected")
    };
    fixture.client.execute(fixture.client.prepare(Command::WritePluginConversationValue {
        key: "goal".into(), expected_revision: 0,
        value: json!({"id":id,"description":"Verify the requested work","state":state,"after":null}),
    }).with_plugin(plugin::Context {
        invocation: None, turn: None, surface: Default::default(),
        package: package.summary.reference(), worktree: Some(fixture.session.worktree),
        session: Some(fixture.session.id),
    })).await.unwrap();
}

#[tokio::test]
async fn commits_results_without_duplicate_turns() {
    for remote in [false, true] {
        for kind in ["pause", "resume", "clear"] {
            let id = sailry_protocol::RequestId::new().to_string();
            let mut calls = vec![(
                plugin_tool("goals", &format!("{kind}_goal")),
                json!({
                    "goal_id":id,"expected_revision":"1",
                }),
            )];
            if kind == "resume" {
                calls.push((plugin_tool("goals", "update_goal"), json!({
                    "goal_id":id,"expected_revision":"2","description":"Verified work","state":"completed",
                })));
            }
            let server = Server::tools(calls).await;
            let fixture = fixture(remote, &server).await;
            seed(
                &fixture,
                &id,
                if kind == "resume" { "paused" } else { "active" },
            )
            .await;
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(submit(
                    &fixture,
                    "Apply the explicitly requested goal control",
                ))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs.len(),
                1,
                "a control must not admit another running turn"
            );
            assert_eq!(page.runs[0].status, Status::Completed);
            assert!(page.queue.items.is_empty());
            let outputs = results(&fixture.client, &page).await;
            assert_eq!(outputs.len(), if kind == "resume" { 2 } else { 1 });
            let controlled = &outputs[0].1["goal"];
            if kind == "clear" {
                assert!(controlled.is_null());
            } else {
                assert_eq!(controlled["id"], id);
                assert_eq!(controlled["revision"], "2");
                assert_eq!(
                    controlled["state"],
                    if kind == "resume" { "active" } else { "paused" }
                );
            }
            assert!(
                outputs
                    .iter()
                    .all(|(_, result)| result.get("error").is_none())
            );
            let value = read(&fixture).await;
            assert_eq!(
                value["state"].as_str(),
                match kind {
                    "pause" => Some("paused"),
                    "resume" => Some("completed"),
                    _ => None,
                }
            );
            let profile = fixture.node.profile().to_owned();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start_empty(profile).await.unwrap();
            let client = Client::new(if remote {
                fixture.controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(history(&client, fixture.session.id).await, page);
            tokio::task::yield_now().await;
            assert_eq!(
                server.requests.lock().unwrap().len(),
                if kind == "resume" { 3 } else { 1 }
            );
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn rejects_stale_targets_and_paused_updates() {
    for remote in [false, true] {
        let id = sailry_protocol::RequestId::new().to_string();
        let wrong = sailry_protocol::RequestId::new().to_string();
        let server = Server::tools(vec![
            (plugin_tool("goals", "resume_goal"), json!({"goal_id":wrong,"expected_revision":"1"})),
            (plugin_tool("goals", "resume_goal"), json!({"goal_id":id,"expected_revision":"2"})),
            (plugin_tool("goals", "update_goal"), json!({"goal_id":id,"expected_revision":"1","description":"Must remain paused","state":"completed"})),
        ]).await;
        let fixture = fixture(remote, &server).await;
        seed(&fixture, &id, "paused").await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(submit(&fixture, "Inspect rejected goal controls"))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        let outputs = results(&fixture.client, &page).await;
        assert_eq!(outputs.len(), 3);
        for (index, (_, result)) in outputs.iter().enumerate() {
            assert_eq!(result["isError"], true);
            let error: sailry_protocol::Fault = serde_json::from_value(result["error"].clone())
                .expect("canonical tool error expected");
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert_eq!(error.message, "plugin callback failed");
            let Some(sailry_protocol::FaultDetails::SystemDiagnostic(report)) = error.details
            else {
                panic!("script diagnostic expected")
            };
            assert_eq!(
                report.source,
                sailry_protocol::notification::SystemSource::PluginCallback
            );
            assert_eq!(
                report.phase,
                sailry_protocol::notification::DiagnosticPhase::Callback
            );
            assert_eq!(
                report.reason,
                sailry_protocol::notification::DiagnosticReason::Failure
            );
            assert_eq!(
                report.exception,
                Some(sailry_protocol::notification::ScriptException::Error)
            );
            assert_eq!(report.code, ErrorCode::InvalidRequest);
            assert_eq!(
                report.plugin.as_ref().map(|plugin| plugin.name.as_str()),
                Some("goals")
            );
            assert_eq!(report.session, Some(fixture.session.id));
            assert_eq!(report.worktree, Some(fixture.session.worktree));
            assert_eq!(report.turn, Some(turn.id));
            let public = result.to_string();
            assert!(!public.contains(if index < 2 {
                "Goal changed; read it before updating"
            } else {
                "No active goal exists"
            }));
            assert!(!public.contains("Exception generated by QuickJS"));
        }
        let value = read(&fixture).await;
        assert_eq!(value["id"], id);
        assert_eq!(value["revision"], "1");
        assert_eq!(value["state"], "paused");
        assert_eq!(page.runs.len(), 1);
        assert!(page.queue.items.is_empty());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
