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
    assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
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
        let fixture = Fixture::new(remote, &server).await;
        std::fs::write(fixture.root.join("source.txt"), "Repeated summary evidence").unwrap();
        for index in 1usize..=6 {
            let text = format!("Turn {index} requirement ").repeat(1100);
            let (_, _, page) = submit(&fixture.client, fixture.session.id, text).await;
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| entry.author == "system")
                    .count(),
                index.saturating_sub(2)
            );
        }
        let requests = server.requests.lock().unwrap().clone();
        let summaries: Vec<_> = requests
            .iter()
            .filter(|request| agent_support::compaction::is_summary(request))
            .collect();
        assert_eq!(summaries.len(), 4);
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
            vec![14000, 14000, 14000, 14000, 100],
        )
        .await;
        let fixture = Fixture::new(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Original tool evidence 中文 🙂",
        )
        .unwrap();
        let original = "Original constraint 中文 🙂 ".repeat(700);
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
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        let summary = requests.last().unwrap();
        assert!(agent_support::compaction::is_summary(summary));
        assert!(
            summary["tools"].is_null() || summary["tools"].as_array().is_some_and(Vec::is_empty)
        );
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
        assert!(
            before
                .entries
                .iter()
                .any(|entry| entry.parts.contains(&Part::Text(original.clone())))
        );
        assert!(
            before
                .entries
                .iter()
                .any(|entry| entry.author == "system" && entry.usage.is_some())
        );
        usage_total(&fixture.client, 5, 4 * 14000 + 100).await;
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
        let node = Node::start(fixture.directory.path().join("node"))
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
        assert_eq!(server.requests.lock().unwrap().len(), 5);
        usage_total(&client, 5, 4 * 14000 + 100).await;
        for session in [fixture.session.id, branch.id] {
            submit(&client, session, "Continue after restart".into()).await;
            let current = server.requests.lock().unwrap().last().unwrap().clone();
            let contents = messages(&current);
            for text in [
                "Context summary fixture",
                "Second recent turn",
                "Third recent turn",
                "Continue after restart",
            ] {
                assert!(contents.contains(text), "{text}");
            }
            assert!(!contents.contains("Original constraint"));
            assert!(!contents.contains("evidence-call"));
        }
        submit(&client, early.id, "Continue early branch".into()).await;
        let current = server.requests.lock().unwrap().last().unwrap().clone();
        assert!(messages(&current).contains("Original constraint"));
        assert!(messages(&current).contains("Original tool evidence 中文 🙂"));
        assert!(!messages(&current).contains("Context summary fixture"));
        assert_eq!(server.requests.lock().unwrap().len(), 8);
        usage_total(&client, 8, 4 * 14000 + 4 * 100).await;
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn failure_preserves_context() {
    for remote in [false, true] {
        let server = Server::automatic_compaction(true).await;
        let fixture = Fixture::new(remote, &server).await;
        std::fs::write(fixture.root.join("source.txt"), "Evidence preserved").unwrap();
        submit(
            &fixture.client,
            fixture.session.id,
            "Original requirement ".repeat(1000),
        )
        .await;
        submit(&fixture.client, fixture.session.id, "Second".into()).await;
        let (_, _, page) = submit(&fixture.client, fixture.session.id, "Third".into()).await;
        assert!(!page.entries.iter().any(|entry| entry.author == "system"));
        assert_eq!(server.requests.lock().unwrap().len(), 5);
        submit(&fixture.client, fixture.session.id, "Continue".into()).await;
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 7);
        let current = &requests[5];
        assert!(messages(current).contains("Original requirement"));
        assert!(messages(current).contains("Evidence preserved"));
        assert!(messages(current).contains("evidence-call"));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
