use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requires_a_completed_answer() {
    for remote in [false, true] {
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
            eprintln!("completion case: remote={remote}, outcome={outcome}");
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path != "/responses" {
                    return server::standard(request, 3600);
                }
                eprintln!("completion inference: remote={remote}, outcome={outcome}");
                server::input(
                    request,
                    &Options::defaults(Authentication::ChatGptOAuth).unwrap(),
                    ACCESS,
                );
                let mut terminal =
                    server::completed(json!([server::message("Authorized Codex response")]));
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
                        terminal["response"]["error"] =
                            json!({"code":"invalid_request","message":ACCESS});
                        server::stream(vec![terminal], true)
                    }
                    "empty" => server::stream(vec![server::completed(json!([]))], true),
                    _ => unreachable!(),
                }
            })
            .await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = configured(&fixture, None).await;
            let session = super::super::execution::session(&fixture, &provider).await;
            let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
            let status = if outcome == "completed" {
                Status::Completed
            } else {
                Status::Failed
            };
            let page = if matches!(outcome, "eof" | "done") {
                super::super::execution::finished_within(
                    &fixture.client,
                    &turn,
                    status,
                    crate::retry_fixture::budget(),
                )
                .await
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
                .filter(|request| request.path == "/responses")
                .count();
            if matches!(outcome, "eof" | "done") {
                assert_eq!(retries, [1, 2, 3, 4, 5], "{outcome}");
                assert_eq!(requests, retries.len() + 1, "{outcome}");
            } else {
                assert!(retries.is_empty(), "{outcome}");
                assert_eq!(requests, 1, "{outcome}: inference must not replay");
            }
            fixture.close().await;
        }
    }
}
