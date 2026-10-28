use super::*;

#[tokio::test]
async fn preserves_native_outcomes() {
    let selected = Arc::new(std::sync::Mutex::new("completed"));
    let outcome = selected.clone();
    let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
        if request.path != "/responses" {
            return server::standard(request, 3600);
        }
        let outcome = *outcome.lock().unwrap();
        server::input(
            request,
            &Options::defaults(Authentication::ChatGptOAuth).unwrap(),
            ACCESS,
        );
        let mut terminal = server::completed(json!([server::message("Authorized Codex response")]));
        match outcome {
            "completed" => server::stream(
                vec![server::delta("Authorized Codex response"), terminal],
                true,
            ),
            "eof" => server::stream(vec![server::delta("Draft")], false),
            "done" => server::stream(vec![server::delta("Draft")], true),
            "incomplete" | "absent" | "unknown" => {
                terminal["type"] = json!("response.incomplete");
                terminal["response"]["status"] = json!("incomplete");
                if outcome != "absent" {
                    terminal["response"]["incomplete_details"] = json!({"reason":if outcome == "unknown" { "fixture_unknown_reason" } else { "max_output_tokens" }});
                }
                server::stream(vec![terminal], true)
            }
            "failed" => {
                server::stream(vec![json!({"type":"error","error":{
                    "code":"invalid_prompt","message":ACCESS
                }})], true)
            }
            "empty" => server::stream(vec![server::completed(json!([]))], true),
            _ => unreachable!(),
        }
    })
    .await;
    let mut fixture = Fixture::empty(false, &server).await;
    let provider = configured(&fixture, None).await;
    let authorization_requests = server.requests.lock().unwrap().len();
    for remote in [false, true] {
        fixture.client = Arc::new(Client::new(if remote {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        } else {
            fixture.node.local()
        }));
        for outcome in [
            "completed",
            "eof",
            "done",
            "incomplete",
            "absent",
            "unknown",
            "failed",
            "empty",
        ] {
            *selected.lock().unwrap() = outcome;
            let before = server.requests.lock().unwrap().len();
            let session = super::super::execution::session(&fixture, &provider).await;
            let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
            let status = if outcome == "failed" {
                Status::Failed
            } else {
                Status::Completed
            };
            let page = super::super::execution::finished_within(
                &fixture.client,
                &turn,
                status,
                crate::retry_fixture::budget(),
            )
            .await;
            if outcome == "failed" {
                assert!(
                    page.runs
                        .iter()
                        .find(|run| run.turn == turn.id)
                        .unwrap()
                        .error
                        .is_some()
                );
            }
            let expected = match outcome {
                "eof" | "done" => "Draft",
                "completed" | "incomplete" | "absent" | "unknown" => "Authorized Codex response",
                "failed" => "",
                "empty" => goose_agent::inference::EMPTY_RESPONSE_MESSAGE,
                _ => unreachable!(),
            };
            assert_eq!(answer(&page, turn.id), expected, "{outcome}");
            if status == Status::Completed {
                assert!(
                    page.runs
                        .iter()
                        .find(|run| run.turn == turn.id)
                        .unwrap()
                        .error
                        .is_none()
                );
            }
            no_secrets(&page);
            let retries: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::Resource(value) if value["type"] == "model_retry" => Some(value),
                    _ => None,
                })
                .collect();
            let requests = server
                .requests
                .lock()
                .unwrap()
                .iter()
                .skip(before)
                .filter(|request| request.path == "/responses")
                .count();
            assert!(retries.is_empty(), "remote={remote} outcome={outcome}");
            // Goose retries an empty inference three times; a received partial
            // answer or provider error is never replayed by Sailry.
            assert_eq!(
                requests,
                if outcome == "empty" { 4 } else { 1 },
                "{outcome}"
            );
        }
    }
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.path != "/responses")
            .count(),
        authorization_requests,
        "response cases must retain the original grant"
    );
    fixture.close().await;
}
