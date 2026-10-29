use super::*;
use process::Fixture;
use serde_json::Value;

#[path = "compaction/active.rs"]
mod active;
#[path = "compaction/budget.rs"]
mod budget;
#[path = "compaction/manual.rs"]
mod manual;

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
        for index in 1usize..=6 {
            let (_, _, page) = submit(
                &fixture.client,
                fixture.session.id,
                format!("Turn {index} requirement ").repeat(100),
            )
            .await;
            assert_eq!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .filter(|part| matches!(part, Part::Compaction(_)))
                    .count(),
                index.saturating_sub(3)
            );
        }
        let requests = server.requests.lock().unwrap().clone();
        for request in &requests {
            if agent_support::compaction::is_summary(request) {
                assert!(!request.to_string().contains("<turn-context>"));
            } else {
                assert!(request.get("turn_context").is_none());
                assert!(agent_support::turn_context(request).is_some());
                assert_eq!(
                    request["messages"]
                        .to_string()
                        .matches("<turn-context>\\n")
                        .count(),
                    1
                );
            }
        }
        observations::assert_context_absent(&fixture.node, fixture.session.id).await;
        let summaries: Vec<_> = requests
            .iter()
            .filter(|request| agent_support::compaction::is_summary(request))
            .collect();
        assert_eq!(summaries.len(), 3);
        for (index, summary) in summaries.iter().enumerate() {
            assert!(summary["tools"].as_array().is_none_or(Vec::is_empty));
            assert!(messages(summary).contains(&format!("Turn {} requirement", index + 4)));
            if index > 0 {
                assert!(messages(summary).contains("Context summary fixture"));
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
            "Context summary fixture: retained source facts".into(),
            vec![100, 100, 100, 26000, 100],
        )
        .await;
        let fixture = fixture(remote, &server).await;
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
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| !agent_support::compaction::is_summary(request))
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
        for session in [fixture.session.id, branch.id] {
            let prior = history(&client, session).await.entries;
            let (_, _, after) = submit(&client, session, "Continue after restart".into()).await;
            assert_eq!(&after.entries[..prior.len()], prior);
            let requests = server.requests.lock().unwrap().clone();
            let summary = &requests[requests.len() - 2];
            assert!(agent_support::compaction::is_summary(summary));
            assert!(summary["tools"].as_array().is_none_or(Vec::is_empty));
            let transcript = messages(summary);
            for text in [
                "Original constraint",
                "Original tool evidence 中文 🙂",
                "read_file",
                "Second recent turn",
                "Third recent turn",
            ] {
                assert!(transcript.contains(text), "{text}");
            }
            let current = messages(requests.last().unwrap());
            assert!(current.contains("Context summary fixture"));
            assert!(current.contains("Continue after restart"));
            assert!(!current.contains("Original tool evidence"));
            assert!(!current.contains("evidence-call"));
        }
        submit(&client, early.id, "Continue early branch".into()).await;
        let current = server.requests.lock().unwrap().last().unwrap().clone();
        assert!(messages(&current).contains("Original constraint"));
        assert!(messages(&current).contains("Original tool evidence 中文 🙂"));
        assert!(!messages(&current).contains("Context summary fixture"));
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[path = "compaction/recovery.rs"]
mod recovery;
#[path = "compaction/statistics.rs"]
mod statistics;
