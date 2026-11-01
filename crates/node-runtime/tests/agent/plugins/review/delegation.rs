use super::*;

#[tokio::test]
async fn flagship_preserves_receipt_choice() {
    for remote in [false, true] {
        for recording in [false, true] {
            let (mut fixture, _) = start(remote).await;
            planning::configure(&mut fixture, WorkMode::Flagship).await;
            let repository = Repository::init(&fixture.root).unwrap();
            write(&fixture.root, "src/value.rs", "old_value();\n");
            commit(&repository, "Initial value");
            write(&fixture.root, "src/value.rs", "new_value();\n");
            let scope = scope(&fixture, json!({"kind":"worktree","data":{"scope":"all"}})).await;
            let child = Server::tools(if recording {
                vec![(
                    plugin_tool(PACKAGE, "review_complete"),
                    receipts(&scope, false),
                )]
            } else {
                vec![]
            })
            .await;
            concurrency::roles(&mut fixture, &[("reviewer", &child)]).await;
            let (results, _) = calls(
                &fixture,
                vec![(
                    plugin_tool(PACKAGE, "delegate_review"),
                    json!({
                        "review_id":scope["review_id"],
                        "groups":[scope["groups"][0]["id"]],
                        "record_receipt":recording,
                        "role":"reviewer",
                    }),
                )],
            )
            .await;
            let result = &results[0];
            success(result);
            assert_eq!(result["admitted"], true, "{result}");
            let session: SessionId = serde_json::from_value(result["session"].clone()).unwrap();
            let turn: TurnId = serde_json::from_value(result["turn"].clone()).unwrap();
            let page = completed(&fixture.client, session, turn).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn)
                    .unwrap()
                    .status,
                Status::Completed
            );
            let output = crate::approvals::rules::results(&fixture.client, &page, turn).await;
            assert_eq!(output.len(), usize::from(recording));
            for (_, _, result) in output {
                success(&result);
            }
            let current = call(
                &fixture,
                "review_scope",
                json!({"review_id":scope["review_id"]}),
            )
            .await;
            success(&current);
            assert_eq!(
                current["reviewed_groups"].as_array().unwrap().len(),
                usize::from(recording)
            );
            assert_eq!(current["status"], "open");
            stop(fixture).await;
        }
    }
}
