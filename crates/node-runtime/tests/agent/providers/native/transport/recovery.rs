use super::*;

#[tokio::test]
async fn preserves_accepted_unfinished_responses() {
    for remote in [false, true] {
        for partial in [false, true] {
            let server = Server::transport(
                ModelApi::Responses,
                Reply::Text,
                Mode::Interrupt {
                    failures: 2,
                    partial,
                    after_tool: false,
                },
            )
            .await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            let turn = fixture.submit("Recover this one turn").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Failed,
                "{:?}",
                page.runs
            );
            assert_eq!(
                completion::response(&page),
                if partial { "FAILED_DRAFT" } else { "" }
            );
            assert_eq!(page.runs.iter().filter(|run| run.turn == turn).count(), 1);
            assert!(super::super::retry::attempts(&page).is_empty());
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 1);
            for request in &requests {
                assert_eq!(request.body["type"], "response.create");
                assert!(request.body.get("previous_response_id").is_none());
                let body = request.body.to_string();
                assert!(!body.contains("FAILED_DRAFT"));
                assert!(!body.contains("failed_"));
                assert_eq!(body.matches("Recover this one turn").count(), 1);
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn preserves_completed_tools_without_replay() {
    for remote in [false, true] {
        let server = Server::transport(
            ModelApi::Responses,
            Reply::Tool,
            Mode::Interrupt {
                failures: 2,
                partial: true,
                after_tool: true,
            },
        )
        .await;
        let fixture =
            Fixture::packages(remote, ModelApi::Responses, &server.endpoint, &["files"]).await;
        std::fs::write(fixture.root.join("native.txt"), "工具内容 🙂").unwrap();
        let turn = fixture.submit("Read native.txt once").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Failed,
            "{:?}",
            page.runs
        );
        assert_eq!(completion::response(&page), "FAILED_DRAFT");
        assert!(super::super::retry::attempts(&page).is_empty());
        let results = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter(|part| matches!(part, Part::ToolResult { .. }))
            .count();
        assert_eq!(results, 1);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        for request in &requests[1..] {
            assert_eq!(request.body["previous_response_id"], "native-response");
            let body = request.body.to_string();
            assert_eq!(body.matches("工具内容 🙂").count(), 1);
            assert!(!body.contains("FAILED_DRAFT"));
            assert!(!body.contains("failed_"));
            assert_eq!(body.matches("Read native.txt once").count(), 0);
        }
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn does_not_replay_completed_provider_tools() {
    for remote in [false, true] {
        let server =
            Server::transport(ModelApi::Responses, Reply::Text, Mode::NativeComplete).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture
            .submit("Preserve the completed provider action")
            .await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Failed);
        assert_eq!(run.error.as_ref().unwrap().code, ErrorCode::OutcomeUnknown);
        assert!(super::super::retry::attempts(&page).is_empty());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
