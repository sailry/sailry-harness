use super::*;

async fn advance_backoff(client: &Client, turn: &QueuedTurn, server: &Server, before: usize) {
    let policy = adk_model::retry::RetryConfig::default().with_max_retries(5);
    let mut updates = client.subscribe_conversation(turn.session).await.unwrap();
    let mut projection =
        sailry_client::conversation::Projection::new(client.target(), turn.session, 0);
    tokio::time::timeout(crate::retry_fixture::budget(), async {
        let attempt = 1_u32;
        loop {
            if projection.snapshot().is_some_and(|snapshot| {
                snapshot
                    .page
                    .entries
                    .iter()
                    .filter(|entry| entry.turn == turn.id)
                    .flat_map(|entry| &entry.parts)
                    .any(|part| {
                        matches!(part, Part::Resource(value)
                            if value["type"] == "model_retry"
                                && value["attempt"].as_u64() == Some(u64::from(attempt)))
                    })
            }) {
                break;
            }
            if projection.apply(0, updates.next().await.unwrap()).unwrap()
                == sailry_client::Apply::Recover
            {
                updates = client.subscribe_conversation(turn.session).await.unwrap();
                projection =
                    sailry_client::conversation::Projection::new(client.target(), turn.session, 0);
            }
        }
        // Finish a real owner round trip after the observer's durable event.
        // A missed timer registration leaves its real backoff in place.
        let Output::Conversation(_) = client
            .execute(client.prepare(Command::ReadConversation {
                session: turn.session,
                before: None,
                limit: 10,
            }))
            .await
            .unwrap()
        else {
            panic!("history expected")
        };
        tokio::task::yield_now().await;
        let requests = server
            .requests
            .lock()
            .unwrap()
            .iter()
            .skip(before)
            .filter(|request| request.path == "/responses")
            .count();
        // A real elapsed backoff may already have opened the next HTTP attempt.
        if requests == attempt as usize {
            tokio::time::pause();
            tokio::time::advance(policy.initial_delay).await;
            tokio::time::resume();
        }
    })
    .await
    .expect("retry observation deadline");
}

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
                terminal["response"]["error"] = json!({"code":"invalid_request","message":ACCESS});
                server::stream(vec![terminal], true)
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
            let status = if outcome == "completed" {
                Status::Completed
            } else {
                Status::Failed
            };
            let page = if matches!(outcome, "eof" | "done") {
                let (_, page) = tokio::join!(
                    advance_backoff(&fixture.client, &turn, &server, before),
                    super::super::execution::finished_within(
                        &fixture.client,
                        &turn,
                        status,
                        crate::retry_fixture::budget(),
                    )
                );
                page
            } else {
                super::super::execution::finished(&fixture.client, &turn, status).await
            };
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
                if outcome == "incomplete" {
                    assert_eq!(answer(&page, turn.id), "Authorized Codex response");
                }
                if matches!(outcome, "absent" | "unknown" | "empty") {
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
                        assert_eq!(value["limit"], 5);
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
                assert!(retries.is_empty(), "{outcome}");
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
