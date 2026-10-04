use super::*;

#[tokio::test]
async fn binds_omitted_session_arguments() {
    let names = ["list_apps", "get_accessibility_tree"];
    let raw = driver::result(json!({"apps":[],"windows":[]}));
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            names
                .iter()
                .map(|name| ((*name).into(), json!({})))
                .collect(),
            WorkMode::Plan,
            Permission::Ask,
            true,
            json!({"list_apps":raw,"get_accessibility_tree":raw}),
        )
        .await;
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(page.approvals.is_empty());
        for name in names {
            assert_eq!(tool_result(&fixture, page.revision, name).await, raw);
        }
        let calls = fixture.worker.calls();
        assert_eq!(calls.len(), names.len());
        assert!(calls.iter().all(|call| call["arguments"] == json!({})));
        let records = fixture.worker.records();
        let binding = records
            .iter()
            .find(|record| record["operation"] == "bind_session")
            .unwrap();
        let invoked: Vec<_> = records
            .iter()
            .filter(|record| {
                record["operation"] == "invoke" && names.contains(&record["tool"].as_str().unwrap())
            })
            .collect();
        assert_eq!(invoked.len(), names.len());
        for invocation in invoked {
            assert_eq!(
                invocation["arguments"],
                json!({"session":binding["arguments"]["public_session"]})
            );
        }
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn refuses_public_session_substitution() {
    let names = ["list_apps", "get_accessibility_tree"];
    let success = driver::result(json!({"apps":[],"windows":[]}));
    let message = "public session substitution does not match the bound authorization context";
    let refusal = json!({"content":[{"type":"text","text":message}],"isError":true,
        "structuredContent":{"status":"refused","refusal":{"code":"permission_denied","message":message}}});
    for remote in [false, true] {
        for label in ["wechat-hello", ""] {
            let arguments = json!({"session":label});
            let fixture = Fixture::new(
                remote,
                names
                    .iter()
                    .map(|name| ((*name).into(), arguments.clone()))
                    .collect(),
                WorkMode::Plan,
                Permission::Ask,
                true,
                json!({"list_apps":success,"get_accessibility_tree":success}),
            )
            .await;
            let page = finished(&fixture.client, fixture.session, fixture.turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert!(page.approvals.is_empty());
            for name in names {
                assert_eq!(tool_result(&fixture, page.revision, name).await, refusal);
            }
            let calls = fixture.worker.calls();
            assert_eq!(calls.len(), names.len());
            assert!(calls.iter().all(|call| call["arguments"] == arguments));
            assert!(
                !fixture
                    .worker
                    .records()
                    .iter()
                    .any(|record| record["operation"] == "invoke"
                        && names.contains(&record["tool"].as_str().unwrap())),
                "refused calls must not reach native fixture outcomes"
            );
            fixture.shutdown().await;
        }
    }
}

#[tokio::test]
async fn preserves_arguments_and_mcp_outcomes() {
    let success = driver::result(
        json!({"snapshot_id":"sdk-snapshot","elements":[],"unknown_extension":{"revision":7}}),
    );
    let refusal = json!({"content":[{"type":"text","text":"Unsupported accessibility action"}],"isError":true,
        "structuredContent":{"code":"ax_action_unsupported","message":"Unsupported accessibility action"}});
    let arguments = json!({"pid":314,"window_id":271,"include_screenshot":false,"query":"Draft 中文","max_depth":7});
    let click = json!({"target":{"kind":"window","pid":314,"window_id":271},"x":12,"y":34,"delivery_mode":"background","button":"left"});
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            vec![
                ("get_window_state".into(), arguments.clone()),
                ("click".into(), click.clone()),
            ],
            WorkMode::Code,
            Permission::Full,
            true,
            json!({"get_window_state":success,"click":refusal}),
        )
        .await;
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        super::super::evaluation::trajectory(
            &page,
            &[
                ("get_window_state".into(), arguments.clone()),
                ("click".into(), click.clone()),
            ],
        );
        assert_eq!(
            tool_result(&fixture, page.revision, "get_window_state").await,
            success
        );
        assert_eq!(tool_result(&fixture, page.revision, "click").await, refusal);
        let calls = fixture.worker.calls();
        assert_eq!(
            calls.len(),
            2,
            "a failure must not dispatch observation, foreground, or fallback tools"
        );
        assert_eq!(calls[0]["name"], "get_window_state");
        assert_eq!(calls[0]["arguments"], arguments);
        assert_eq!(calls[1]["name"], "click");
        assert_eq!(calls[1]["arguments"], click);
        assert!(calls[0]["session_handle"].is_string());
        assert_eq!(calls[0]["session_handle"], calls[1]["session_handle"]);
        {
            let requests = fixture.server.requests.lock().unwrap();
            let final_messages = requests.last().unwrap()["messages"].as_array().unwrap();
            let results: Vec<Value> = final_messages
                .iter()
                .filter(|message| message["role"] == "tool")
                .map(|message| serde_json::from_str(message["content"].as_str().unwrap()).unwrap())
                .collect();
            assert_eq!(results, vec![success.clone(), refusal.clone()]);
        }
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn waits_for_exact_input_authority() {
    let arguments = json!({"target":{"kind":"window","pid":314,"window_id":271},"delivery_mode":"background","x":12,"y":34});
    for remote in [false, true] {
        for (disable, decision) in [
            (false, Decision::Deny),
            (false, Decision::Approve),
            (true, Decision::Approve),
        ] {
            let fixture = Fixture::new(
                remote,
                vec![("click".into(), arguments.clone())],
                WorkMode::Code,
                Permission::Ask,
                true,
                json!({}),
            )
            .await;
            let (page, approval) =
                super::super::approvals::pending(&fixture.client, fixture.session).await;
            assert!(
                matches!(&page.entries.iter().find(|entry| entry.id == approval.entry).unwrap().parts[approval.index],
                Part::ToolCall { name, arguments: actual, .. } if name == "click" && actual == &arguments)
            );
            assert!(fixture.worker.calls().is_empty());
            if disable {
                set_enabled(&fixture.client, false).await;
            }
            fixture
                .client
                .execute(fixture.client.prepare(Command::ResolveApproval {
                    session: fixture.session,
                    approval: approval.id,
                    decision,
                }))
                .await
                .unwrap();
            let page = finished(&fixture.client, fixture.session, fixture.turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            let result = tool_result(&fixture, page.revision, "click").await;
            if !disable && decision == Decision::Approve {
                assert_eq!(result, driver::action());
                assert_eq!(fixture.worker.calls().len(), 1);
                assert_eq!(fixture.worker.calls()[0]["arguments"], arguments);
            } else {
                assert!(result.get("error").is_some(), "{result}");
                assert!(
                    fixture.worker.calls().is_empty(),
                    "denied or disabled calls must not reach the worker"
                );
            }
            fixture.shutdown().await;
        }
    }
}

#[tokio::test]
async fn enforces_frozen_plan_admission() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote,vec![("ask_user".into(),json!({"prompt":"Continue the plan","input":{"kind":"text","multiline":false,"max_bytes":1024}}))],
            WorkMode::Plan,Permission::Full,true,json!({})).await;
        let (_, question) =
            super::super::questions::pending(&fixture.client, fixture.session).await;
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPlugin {
                name: "computer".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("computer package expected")
        };
        let Output::Snapshot(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id == fixture.session)
            .unwrap();
        let mut config = session.config.clone();
        config.mode = WorkMode::Code;
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap();
        let context = plugin::Context {
            invocation: None,
            turn: Some(fixture.turn),
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(session.worktree),
            session: Some(session.id),
        };
        let original = fixture
            .client
            .prepare(Command::UseComputer {
                session: session.id,
                worktree: session.worktree,
                name: "click".into(),
                arguments: json!({"target":{"kind":"window","pid":314,"window_id":271},"delivery_mode":"background","x":12,"y":34}),
            })
            .with_plugin(context);
        let fault = fixture.client.execute(original.clone()).await.unwrap_err();
        assert_eq!(fault.code, ErrorCode::PermissionDenied);
        assert!(fault.message.contains("planning turns"), "{fault:?}");
        assert_eq!(
            fixture.client.outcome(&original).await.unwrap(),
            RequestOutcome::Completed(Box::new(Err(fault)))
        );
        assert!(
            fixture.worker.calls().is_empty(),
            "current Code defaults must not authorize the frozen Plan turn"
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveQuestion {
                session: fixture.session,
                question: question.id,
                response: question::Response::Cancel,
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(page.approvals.is_empty());
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn keeps_one_session_binding_per_owner() {
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            vec![("list_windows".into(), json!({"on_screen_only":false}))],
            WorkMode::Code,
            Permission::Full,
            true,
            json!({"list_windows":driver::result(json!({"windows":[]}))}),
        )
        .await;
        finished(&fixture.client, fixture.session, fixture.turn).await;
        let records = fixture.worker.records();
        assert_eq!(
            records
                .iter()
                .filter(|request| request["operation"] == "initialize")
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .filter(|request| request["operation"] == "bind_session")
                .count(),
            1
        );
        assert_eq!(fixture.worker.calls().len(), 1);
        assert!(
            !records
                .iter()
                .any(|request| request["name"] == "start_session")
        );
        assert_eq!(
            records
                .iter()
                .filter(|request| request["name"] == "end_session")
                .count(),
            0
        );
        assert_eq!(
            records
                .iter()
                .filter(|request| request["operation"] == "close_session")
                .count(),
            0
        );
        drop(fixture.client);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        let records = fixture.worker.records();
        assert_eq!(
            records
                .iter()
                .filter(|request| request["operation"] == "call" && request["name"] == "end_session")
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .filter(|request| request["operation"] == "close_session")
                .count(),
            1
        );
    }
}
