use super::*;

#[tokio::test]
async fn shared_turns_append_changed_guides_once() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::ChatCompletions, Reply::Text).await;
        let fixture = Fixture::packages(
            remote,
            ModelApi::ChatCompletions,
            &server.endpoint,
            &["files"],
        )
        .await;
        prepare(&fixture);
        let changed = "Changed nested fixture rule: preserve current content";
        let other = Client::new(fixture.node.local());
        for (index, client) in [&fixture.client, &other, &fixture.client]
            .into_iter()
            .enumerate()
        {
            if index == 1 {
                std::fs::write(fixture.root.join("nested/GEMINI.md"), changed).unwrap();
            }
            let Output::QueuedTurn(turn) = execute(
                client,
                Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: referenced("Read the current nested guide", false),
                },
            )
            .await
            else {
                panic!("turn expected")
            };
            let page = finished(client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
        }
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for (index, request) in requests.iter().enumerate() {
            let captured = request.body.to_string();
            assert_eq!(captured.matches(ROOT_RULE).count(), 1);
            assert_eq!(captured.matches(NESTED_RULE).count(), 1);
            assert_eq!(captured.matches(changed).count(), usize::from(index > 0));
            assert_eq!(
                instruction(ModelApi::ChatCompletions, &request.body),
                instruction(ModelApi::ChatCompletions, &requests[0].body)
            );
            assert_eq!(request.body["tools"], requests[0].body["tools"]);
        }
        drop(requests);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
