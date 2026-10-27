use super::*;
use process::Fixture;
use serde_json::Value;

#[path = "compaction/active.rs"]
mod active;
#[path = "compaction/budget.rs"]
mod budget;
#[path = "compaction/manual.rs"]
mod manual;
#[path = "compaction/native.rs"]
mod native;

async fn fixture(remote: bool, server: &Server) -> Fixture {
    let fixture = Fixture::files(remote, server).await;
    let mut provider = configuration::snapshot(&fixture.client).await.providers[0].clone();
    // Fit the immutable tool catalog while keeping compaction fixtures small.
    provider.models[0].context = 64_000;
    provider.models[0].compaction_tokens = Some(24_000);
    fixture
        .client
        .execute(fixture.client.prepare(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        }))
        .await
        .unwrap();
    fixture
}

async fn submit(client: &Client, session: SessionId, text: String) -> (Request, TurnId, Page) {
    let request = client.prepare(Command::SubmitTurn {
        session,
        expected_revision: 1,
        message: text.into(),
    });
    let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
        panic!("turn expected")
    };
    let page = finished(client, session, turn.id).await;
    let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
    assert_eq!(run.status, Status::Completed, "{:?}", page.runs);
    (request, turn.id, page)
}

fn messages(request: &Value) -> String {
    request["messages"].to_string()
}

async fn usage(client: &Client, responses: u64) {
    usage_total(client, responses, responses * 100).await;
}

async fn usage_total(client: &Client, responses: u64, input: u64) {
    let now = chrono::Utc::now().timestamp_millis();
    let Output::Usage(report) = client
        .execute(
            client.prepare(Command::ReadUsage(sailry_protocol::usage::Query {
                start_ms: now - 86_400_000,
                end_ms: now + 86_400_000,
                dimension: sailry_protocol::usage::Dimension::Model,
                utc_offset_minutes: 0,
                projects: vec![],
                worktrees: vec![],
                providers: vec![],
                models: vec![],
                before: None,
            })),
        )
        .await
        .unwrap()
    else {
        panic!("usage expected")
    };
    assert_eq!(report.totals.responses, responses);
    let tokens = report.totals.tokens.unwrap();
    assert_eq!(tokens.input, input);
    assert_eq!(tokens.output, responses * 10);
}

#[tokio::test]
async fn incorporates_previous_summaries() {
    for remote in [false, true] {
        let server = Server::automatic_compaction(false).await;
        let fixture = fixture(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Repeated summary evidence\n".repeat(400),
        )
        .unwrap();
        // Each summary retains the latest instruction verbatim, so the replaced
        // turn must hold more than twice the material it keeps.
        for (index, repeats) in (1usize..=6).zip([200, 100, 50, 30, 30, 30]) {
            let text = format!("Turn {index} requirement ").repeat(repeats);
            let (_, _, page) = submit(&fixture.client, fixture.session.id, text).await;
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| {
                        entry
                            .parts
                            .iter()
                            .any(|part| matches!(part, Part::Compaction(_)))
                    })
                    .count(),
                // Turn 3 reports the high usage; Turn 4 applies the first summary.
                index.saturating_sub(3)
            );
        }
        let requests = server.requests.lock().unwrap().clone();
        let summaries: Vec<_> = requests
            .iter()
            .filter(|request| agent_support::compaction::is_summary(request))
            .collect();
        assert_eq!(summaries.len(), 3);
        for (index, summary) in summaries.iter().enumerate() {
            let transcript = messages(summary);
            assert!(transcript.contains(&format!("Turn {} requirement", index + 1)));
            assert!(!transcript.contains(&format!("Turn {} requirement", index + 2)));
            if index > 0 {
                assert!(transcript.contains("Context summary fixture"));
            }
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn resumes_compacted_context() {
    for remote in [false, true] {
        let server = Server::compaction_sequence(
            false,
            "Context summary fixture: retain user constraints and source.txt evidence 中文 🙂"
                .into(),
            vec![100, 100, 100, 26000, 100],
        )
        .await;
        let fixture = fixture(remote, &server).await;
        // The summarized turn must shrink by half while its instruction stays verbatim.
        std::fs::write(
            fixture.root.join("source.txt"),
            "Original tool evidence 中文 🙂\n".repeat(200),
        )
        .unwrap();
        let original = "Original constraint 中文 🙂 ".repeat(100);
        let (_, first, _) = submit(&fixture.client, fixture.session.id, original.clone()).await;
        submit(
            &fixture.client,
            fixture.session.id,
            "Second recent turn".into(),
        )
        .await;
        let (request, third, before) = submit(
            &fixture.client,
            fixture.session.id,
            "Third recent turn".into(),
        )
        .await;
        // The high usage of the third turn is compacted at the next model boundary.
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| !agent_support::compaction::is_summary(request))
        );
        assert!(
            before
                .entries
                .iter()
                .any(|entry| entry.parts.contains(&Part::Text(original.clone())))
        );
        usage_total(&fixture.client, 4, 26000 + 3 * 100).await;
        let Output::Session(branch) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: third,
                expected_revision: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        let Output::Session(early) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: first,
                expected_revision: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(fixture.directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            history(&client, fixture.session.id).await.entries,
            before.entries
        );
        client.execute(request).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        usage_total(&client, 4, 26000 + 3 * 100).await;
        for session in [fixture.session.id, branch.id] {
            submit(&client, session, "Continue after restart".into()).await;
            let requests = server.requests.lock().unwrap().clone();
            let summary = &requests[requests.len() - 2];
            assert!(agent_support::compaction::is_summary(summary));
            let current = requests.last().unwrap().clone();
            assert!(!agent_support::compaction::is_summary(&current));
            assert_eq!(summary["tools"], current["tools"]);
            let transcript = messages(summary);
            for text in [
                "Original constraint",
                "Original tool evidence 中文 🙂",
                "read_file",
                "evidence-call",
            ] {
                assert!(transcript.contains(text), "{text}");
            }
            assert!(!transcript.contains("Second recent turn"));
            assert!(!transcript.contains("Third recent turn"));
            let contents = messages(&current);
            for text in [
                "Context summary fixture",
                "Second recent turn",
                "Third recent turn",
                "Continue after restart",
            ] {
                assert!(contents.contains(text), "{text}");
            }
            assert!(
                contents.contains(&original),
                "original user instruction must remain verbatim"
            );
            assert!(!contents.contains("evidence-call"));
        }
        submit(&client, early.id, "Continue early branch".into()).await;
        let current = server.requests.lock().unwrap().last().unwrap().clone();
        assert!(messages(&current).contains("Original constraint"));
        assert!(messages(&current).contains("Original tool evidence 中文 🙂"));
        assert!(!messages(&current).contains("Context summary fixture"));
        assert_eq!(server.requests.lock().unwrap().len(), 9);
        usage_total(&client, 9, 26000 + 8 * 100).await;
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

async fn submit_retrying(
    client: &Client,
    session: SessionId,
    input: &str,
    server: &Server,
) -> Page {
    use futures::FutureExt as _;
    let request = client.prepare(Command::SubmitTurn {
        session,
        expected_revision: 1,
        message: input.into(),
    });
    let Output::QueuedTurn(turn) = client.execute(request).await.unwrap() else {
        panic!("turn expected")
    };
    let result = std::panic::AssertUnwindSafe(finished_within(
        client,
        session,
        turn.id,
        crate::retry_fixture::budget(),
    ))
    .catch_unwind()
    .await;
    let page = result.unwrap_or_else(|error| {
        let requests = server.requests.lock().unwrap();
        let summaries = requests.iter().filter(|request| agent_support::compaction::is_summary(request)).count();
        eprintln!("Synthetic compaction completion failed: turn={}, summary_requests={}, ordinary_requests={}", turn.id, summaries, requests.len() - summaries);
        std::panic::resume_unwind(error)
    });
    let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
    assert_eq!(run.status, Status::Completed, "{:?}", page.runs);
    page
}

#[tokio::test]
async fn failure_preserves_context() {
    for remote in [false, true] {
        let server = Server::automatic_compaction(true).await;
        let fixture = fixture(remote, &server).await;
        let Output::Providers(providers) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers
            .into_iter()
            .find(|provider| provider.id == fixture.session.config.provider)
            .unwrap();
        provider.request_max_retries = None;
        provider.stream_max_retries = Some(5);
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        std::fs::write(fixture.root.join("source.txt"), "Evidence preserved").unwrap();
        submit(
            &fixture.client,
            fixture.session.id,
            "Original requirement ".repeat(1000),
        )
        .await;
        submit(&fixture.client, fixture.session.id, "Second".into()).await;
        // The third response reports the high usage; the next boundary compacts.
        submit(&fixture.client, fixture.session.id, "Third".into()).await;
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        let page = submit_retrying(&fixture.client, fixture.session.id, "Continue", &server).await;
        let parts = page.entries.iter().flat_map(|entry| &entry.parts);
        assert!(
            !parts
                .clone()
                .any(|part| matches!(part, Part::Compaction(_)))
        );
        assert!(
            parts
                .clone()
                .any(|part| matches!(part, Part::Resource(value)
            if value["type"] == "compaction_failure" && value["failure"]["continued"] == true))
        );
        let retries = parts
            .filter(|part| matches!(part, Part::Resource(value) if value["type"] == "model_retry" && value["limit"] == 5))
            .count();
        assert!(retries > 0);
        let attempted = {
            let requests = server.requests.lock().unwrap();
            let attempts = requests
                .iter()
                .filter(|request| agent_support::compaction::is_summary(request))
                .count();
            assert_eq!(attempts, retries + 1);
            assert_eq!(
                attempts, 2,
                "one summary attempt must share one recovery request"
            );
            assert_eq!(requests.len() - attempts, 5);
            attempts
        };
        let continued = submit_retrying(
            &fixture.client,
            fixture.session.id,
            "Continue again",
            &server,
        )
        .await;
        assert!(
            !continued
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Compaction(_)))
        );
        let requests = server.requests.lock().unwrap().clone();
        let attempts = requests
            .iter()
            .filter(|request| agent_support::compaction::is_summary(request))
            .count();
        assert_eq!(attempts, attempted * 2);
        assert_eq!(requests.len() - attempts, 6);
        let current = requests
            .iter()
            .rfind(|request| !agent_support::compaction::is_summary(request))
            .expect("conversation request expected");
        let contents = messages(current);
        for text in [
            "Continue again",
            "Original requirement",
            "Evidence preserved",
            "evidence-call",
        ] {
            assert!(contents.contains(text), "{text}");
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[path = "compaction/recovery.rs"]
mod recovery;

#[path = "compaction/statistics.rs"]
mod statistics;
