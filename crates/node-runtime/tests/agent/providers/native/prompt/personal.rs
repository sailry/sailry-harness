use super::*;
use sailry_protocol::conversation::Decision;

#[tokio::test]
async fn freezes_current_turn_and_refreshes_next() {
    for remote in [false, true] {
        let server = crate::agent_support::Server::tools(vec![(
            crate::agent_support::plugin_tool("files", "write_file"),
            json!({"path":"result.txt", "text":"Approved fixture change", "expected_revision":null}),
        )]).await;
        let fixture = Fixture::new(remote, ModelApi::ChatCompletions, &server.endpoint).await;
        prepare(&fixture);
        let turn = fixture.submit(FIRST).await;
        let (_, approval) = crate::approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(!fixture.root.join("result.txt").exists());
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_policy(ModelApi::ChatCompletions, &requests[0], false, false);
        }
        std::fs::write(
            fixture.node.profile().join("instructions.md"),
            PERSONAL_NEXT,
        )
        .unwrap();
        execute(
            &fixture.client,
            Command::ResolveApproval {
                session: fixture.session.id,
                approval: approval.id,
                decision: Decision::Approve,
            },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("result.txt")).unwrap(),
            "Approved fixture change"
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_policy(ModelApi::ChatCompletions, &requests[1], false, false);
            assert_eq!(
                instruction(ModelApi::ChatCompletions, &requests[0]),
                instruction(ModelApi::ChatCompletions, &requests[1])
            );
        }
        let next = fixture.submit(SECOND).await;
        let page = finished(&fixture.client, fixture.session.id, next).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert_policy(ModelApi::ChatCompletions, &requests[2], true, false);
        }
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn fails_invalid_preferences_before_model_request() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::ChatCompletions, Reply::Text).await;
        let fixture = Fixture::new(remote, ModelApi::ChatCompletions, &server.endpoint).await;
        let path = fixture.node.profile().join("instructions.md");
        let bytes = b"Invalid\0personal instructions";
        std::fs::write(&path, bytes).unwrap();
        let turn = fixture.submit(FIRST).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Failed, "{page:?}");
        assert!(server.requests.lock().unwrap().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
