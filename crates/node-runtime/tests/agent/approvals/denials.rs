#![cfg(unix)]
use super::rules::{claimed, completed, execute, pending, restart, results, submit};
use super::*;
use process::Fixture;
use sailry_protocol::conversation::ApprovalChoice;
use serde_json::{Value, json};

fn tool_messages(request: &Value) -> Vec<(String, Value)> {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| {
            (
                message["tool_call_id"].as_str().unwrap().into(),
                serde_json::from_str(message["content"].as_str().unwrap()).unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn preserves_reasons_without_crossing_turns() {
    for remote in [false, true] {
        let tool = plugin_tool("files", "write_file");
        let arguments = json!({"path":"effect.txt","text":"Approved only in the third turn","expected_revision":null});
        let server = Server::turn_tools(vec![(tool.clone(), arguments)]).await;
        let mut fixture = Fixture::new(remote, &server).await;
        let first = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        let original = completed(&fixture.client, fixture.session.id, first.id).await;
        let first_result = results(&original, first.id).pop().unwrap();
        assert!(
            first_result.2["error"]
                .as_str()
                .unwrap()
                .contains("execution denied by confirmation policy")
        );
        assert_eq!(original.approvals[0].reason, None);
        assert!(!fixture.root.join("effect.txt").exists());

        let second = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        for reason in [" \t\n".into(), "invalid\0reason".into(), "🙂".repeat(2001)] {
            let error = fixture
                .client
                .execute(fixture.client.prepare(Command::ResolveApproval {
                    session: fixture.session.id,
                    approval: approval.id,
                    decision: Decision::DenyWithReason(reason),
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert_eq!(pending(&fixture.client, fixture.session.id).await, approval);
            assert!(!fixture.root.join("effect.txt").exists());
        }
        let reason = "🙂".repeat(2000);
        let (decision, receipt) = process::decide(
            &fixture.client,
            &approval,
            Decision::DenyWithReason(reason.clone()),
        )
        .await;
        let denied = completed(&fixture.client, fixture.session.id, second.id).await;
        let second_result = results(&denied, second.id).pop().unwrap();
        assert_eq!(first_result.0, second_result.0);
        assert_eq!(first_result.1, second_result.1);
        assert_eq!(
            second_result.2,
            json!({"error":format!("Tool execution was denied. Reason: {reason}")})
        );
        let retained = denied
            .approvals
            .iter()
            .find(|item| item.id == approval.id)
            .unwrap();
        assert_eq!(retained.reason.as_deref(), Some(reason.as_str()));
        assert_eq!(retained.state, ApprovalState::Denied);
        assert!(!claimed(&fixture.node, &approval).1);
        assert!(!fixture.root.join("effect.txt").exists());
        let requests = server.requests.lock().unwrap().clone();
        let messages = tool_messages(&requests[3]);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].1, first_result.2);
        assert_eq!(messages[1].1, second_result.2);
        assert_eq!(messages[0].0, messages[1].0);

        fixture = restart(fixture, remote).await;
        assert_eq!(history(&fixture.client, fixture.session.id).await, denied);
        assert_eq!(fixture.client.execute(decision).await.unwrap(), receipt);
        let third = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        let page = completed(&fixture.client, fixture.session.id, third.id).await;
        let third_result = results(&page, third.id).pop().unwrap();
        assert_eq!(first_result.0, third_result.0);
        assert_eq!(first_result.1, third_result.1);
        assert!(third_result.2.get("error").is_none());
        assert!(third_result.2.get("isError").is_none());
        assert_eq!(results(&page, first.id).pop().unwrap(), first_result);
        assert_eq!(results(&page, second.id).pop().unwrap(), second_result);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("effect.txt")).unwrap(),
            "Approved only in the third turn"
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 6);
        let messages = tool_messages(&requests[5]);
        assert_eq!(
            messages
                .iter()
                .map(|(_, response)| response)
                .collect::<Vec<_>>(),
            vec![&first_result.2, &second_result.2, &third_result.2]
        );
        assert!(
            messages
                .iter()
                .all(|(id, _)| Some(id) == first_result.0.as_ref())
        );
        fixture = restart(fixture, remote).await;
        assert_eq!(history(&fixture.client, fixture.session.id).await, page);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn binds_closed_choices_and_durable_retries() {
    for remote in [false, true] {
        for decision in [
            Decision::Approve,
            Decision::ApproveSession,
            Decision::ApprovePrefix("touch".into()),
            Decision::Deny,
            Decision::DenyWithReason("Keep the original file 🙂".into()),
        ] {
            let server = Server::tools(vec![(
                plugin_tool("commands", "run_command"),
                json!({"command":"touch effect.txt"}),
            )])
            .await;
            let mut fixture = Fixture::new(remote, &server).await;
            let turn = submit(&fixture.client, &fixture.session).await;
            let approval = pending(&fixture.client, fixture.session.id).await;
            let wrong = fixture.client.prepare(Command::ResolveApproval {
                session: SessionId::new(),
                approval: approval.id,
                decision: decision.clone(),
            });
            assert_eq!(
                fixture.client.execute(wrong).await.unwrap_err().code,
                ErrorCode::WrongTarget
            );
            let (request, receipt) =
                process::decide(&fixture.client, &approval, decision.clone()).await;
            let page = completed(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                fixture.client.execute(request.clone()).await.unwrap(),
                receipt
            );
            assert_eq!(
                execute(
                    &fixture.client,
                    Command::ResolveApproval {
                        session: fixture.session.id,
                        approval: approval.id,
                        decision: decision.clone(),
                    }
                )
                .await,
                receipt
            );
            let retained = page
                .approvals
                .iter()
                .find(|item| item.id == approval.id)
                .unwrap();
            let allowed = matches!(
                &decision,
                Decision::Approve | Decision::ApproveSession | Decision::ApprovePrefix(_)
            );
            assert_eq!(fixture.root.join("effect.txt").exists(), allowed);
            assert_eq!(claimed(&fixture.node, &approval).1, allowed);
            if decision == Decision::Approve {
                assert_eq!(retained.choice, Some(ApprovalChoice::Once));
            }
            let different = match &decision {
                Decision::Approve => Decision::ApproveSession,
                Decision::ApproveSession => Decision::Approve,
                Decision::ApprovePrefix(_) => Decision::ApprovePrefix("touch effect.txt".into()),
                Decision::Deny => Decision::DenyWithReason("Additional reason".into()),
                Decision::DenyWithReason(_) => {
                    Decision::DenyWithReason("A different reason".into())
                }
            };
            for reused in [true, false] {
                let mut conflict = if reused {
                    request.clone()
                } else {
                    fixture.client.prepare(Command::ResolveApproval {
                        session: fixture.session.id,
                        approval: approval.id,
                        decision: different.clone(),
                    })
                };
                conflict.command = Command::ResolveApproval {
                    session: fixture.session.id,
                    approval: approval.id,
                    decision: different.clone(),
                };
                assert_eq!(
                    fixture.client.execute(conflict).await.unwrap_err().code,
                    ErrorCode::Conflict
                );
            }
            fixture = restart(fixture, remote).await;
            assert_eq!(history(&fixture.client, fixture.session.id).await, page);
            assert_eq!(
                fixture.client.execute(request.clone()).await.unwrap(),
                receipt
            );
            let mut conflict = request;
            conflict.command = Command::ResolveApproval {
                session: fixture.session.id,
                approval: approval.id,
                decision: different,
            };
            assert_eq!(
                fixture.client.execute(conflict).await.unwrap_err().code,
                ErrorCode::Conflict
            );
            assert_eq!(server.requests.lock().unwrap().len(), 2);
            assert_eq!(fixture.root.join("effect.txt").exists(), allowed);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
