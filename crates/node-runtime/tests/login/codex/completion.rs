use super::*;

#[tokio::test]
async fn requires_a_completed_answer() {
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
            "completed" => server::stream(vec![server::delta("Draft"), terminal], true),
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
                terminal["type"] = json!("response.failed");
                terminal["response"]["status"] = json!("failed");
                terminal["response"]["output"] = json!([]);
                terminal["response"]["error"] = json!({"code":"invalid_prompt","message":ACCESS});
                server::stream(vec![terminal], true)
            }
            "empty" => server::stream(vec![server::completed(json!([]))], true),
            _ => unreachable!(),
        }
    })
    .await;
    let mut fixture = Fixture::empty(false, &server).await;
    fixture
        .node
        .set_model_transport_fixture(adk_model::openai::WsOptions {
            enabled: Arc::new(|| false),
            ..Default::default()
        });
    let mut provider = configured(&fixture, None).await;
    provider.stream_max_retries = Some(1);
    let Output::Provider(provider) = fixture
        .client
        .execute(fixture.client.prepare(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
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
            let status = if outcome == "completed" {
                Status::Completed
            } else {
                Status::Failed
            };
            let page = super::super::execution::finished_within(
                &fixture.client,
                &turn,
                status,
                crate::retry_fixture::budget(),
            )
            .await;
            if outcome == "completed" {
                assert_eq!(answer(&page, turn.id), "Authorized Codex response");
            } else {
                assert!(
                    page.runs
                        .iter()
                        .find(|run| run.turn == turn.id)
                        .unwrap()
                        .error
                        .is_some()
                );
                if matches!(outcome, "incomplete" | "absent" | "unknown" | "empty") {
                    assert!(answer(&page, turn.id).is_empty());
                }
            }
            no_secrets(&page);
            let retries: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::Resource(value) if value["type"] == "model_retry" => {
                        assert_eq!(value["limit"], 1);
                        Some(value["attempt"].as_u64().unwrap())
                    }
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
            if matches!(outcome, "eof" | "done") {
                assert_eq!(retries, [1], "{outcome}");
                assert_eq!(requests, retries.len() + 1, "{outcome}");
            } else {
                assert!(
                    retries.is_empty(),
                    "remote={remote} outcome={outcome} retries={retries:?}"
                );
                assert_eq!(requests, 1, "{outcome}: inference must not replay");
            }
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
