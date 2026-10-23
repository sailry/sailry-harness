use super::*;

#[tokio::test]
async fn records_response_starts_on_both_transports() {
    for remote in [false, true] {
        for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
            let socket = api == ModelApi::Responses;
            let server = if socket {
                Server::transport(api, Reply::Text, Mode::Accept).await
            } else {
                Server::start(api, Reply::Text).await
            };
            let fixture = Fixture::empty(remote, api, &server.endpoint).await;
            for index in 0..2 {
                let turn = fixture.submit("Measure this response").await;
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                let statistics =
                    crate::observations::statistics(&fixture.client, fixture.session.id).await;
                let observation = statistics.observations.recent.last().unwrap();
                assert_eq!(observation.turn, turn);
                assert_eq!(observation.turn_index, Some(index + 1));
                let attempt = observation.attempts.last().unwrap();
                let received = attempt.response_us.expect("API response receipt");
                assert!(received <= attempt.first_text_us.unwrap());
                assert!(received <= attempt.elapsed_us.unwrap());
                if socket {
                    assert_eq!(attempt.transport, "web_socket");
                    if index == 1 {
                        assert_eq!(attempt.reused, Some(true));
                        assert_eq!(attempt.headers_us, None);
                    }
                } else {
                    assert_eq!(attempt.transport, "http");
                    assert_eq!(attempt.headers_us, Some(received));
                }
                assert_eq!(
                    crate::observations::statistics(&fixture.client, fixture.session.id).await,
                    statistics
                );
                assert!(
                    !serde_json::to_string(observation)
                        .unwrap()
                        .contains(server::KEY)
                );
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn retains_receipts_without_changing_outcomes() {
    use sailry_protocol::conversation::observation::Outcome;
    for remote in [false, true] {
        let server = Server::transport(ModelApi::Responses, Reply::Text, Mode::ErrorOnce).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Recover the provider rejection").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let statistics = crate::observations::statistics(&fixture.client, fixture.session.id).await;
        let observation = statistics.observations.recent.last().unwrap();
        assert_eq!(statistics.turns, 1);
        assert_eq!(observation.turn, turn);
        assert_eq!(observation.turn_index, Some(1));
        assert_eq!(observation.attempts.len(), 2);
        assert_eq!(observation.attempts[0].outcome, Outcome::Failed);
        assert!(observation.attempts[0].response_us.is_some());
        assert_eq!(observation.attempts[1].outcome, Outcome::Succeeded);
        assert!(observation.attempts[1].response_us.is_some());
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
