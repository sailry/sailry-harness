use super::*;
use sailry_protocol::tool::{MAX_OUTPUT_PAGE_BYTES, OutputReference};

pub(super) fn assert_content(request: &Value, markers: &[&str]) {
    let messages = request["messages"].as_array().unwrap();
    let system = messages
        .iter()
        .filter(|message| matches!(message["role"].as_str(), Some("system" | "developer")))
        .map(|message| message["content"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    for marker in markers {
        assert_eq!(system.matches(marker).count(), 0, "system: {marker}");
        assert_eq!(
            crate::agent_support::turn_contexts(request)
                .iter()
                .map(|context| context.matches(marker).count())
                .sum::<usize>(),
            1,
            "owner: {marker}"
        );
        assert_eq!(
            request["messages"].to_string().matches(marker).count(),
            1,
            "request: {marker}"
        );
    }
}

pub(super) fn receipt(request: &Value, id: &str) -> Value {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
        .map(|message| serde_json::from_str(message["content"].as_str().unwrap()).unwrap())
        .unwrap()
}

pub(super) fn owner(request: &Value, path: &str) -> Value {
    // Owner parts persist once; a later copy replaces an earlier version.
    crate::agent_support::turn_contexts(request)
        .iter()
        .flat_map(|context| context.lines())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value["skill"] == "example:analysis" && value["path"] == path)
        .last()
        .unwrap()
}

pub(super) async fn original(client: &Client, turn: TurnId, receipt: &Value) -> Value {
    assert!(receipt.get("content").is_none());
    assert_eq!(
        receipt["context"],
        "Full content is supplied in retained owner context"
    );
    let output: OutputReference = serde_json::from_value(receipt["output"].clone()).unwrap();
    let mut offset = 0;
    let mut text = String::new();
    loop {
        let Output::ToolOutput(page) = execute(
            client,
            Command::ReadToolOutput {
                turn,
                output: output.clone(),
                stream: None,
                offset,
                limit: MAX_OUTPUT_PAGE_BYTES,
            },
        )
        .await
        else {
            panic!("original result page expected")
        };
        assert_eq!(page.output, output);
        assert_eq!(page.offset, offset);
        text.push_str(&page.text);
        if let Some(next) = page.next_offset {
            assert!(next > offset);
            offset = next;
        } else {
            assert_eq!(page.total_bytes, text.len());
            break;
        }
    }
    assert_eq!(
        output.revision,
        blake3::hash(text.as_bytes()).to_hex().to_string()
    );
    serde_json::from_str(&text).unwrap()
}

async fn route(client: &Client, session: &Session, server: &Server) {
    let Output::Providers(providers) = execute(client, Command::ListProviders).await else {
        panic!("providers expected")
    };
    let mut provider = providers
        .into_iter()
        .find(|provider| provider.id == session.config.provider)
        .unwrap();
    provider.endpoint = server.endpoint.clone();
    let revision = provider.revision;
    execute(
        client,
        Command::PutProvider {
            provider,
            expected_revision: revision,
        },
    )
    .await;
}

#[tokio::test]
async fn restores_loaded_content_after_compaction() {
    for remote in [false, true] {
        let gate = Arc::new(tokio::sync::Notify::new());
        let server = Server::held_completion(
            vec![
                ("load_skill".into(), json!({"skill":"example:analysis"})),
                (
                    "read_skill_resource".into(),
                    json!({"skill":"example:analysis","path":"references/guide.md"}),
                ),
            ],
            gate.clone(),
        )
        .await;
        let (_directory, node, controller, client, session) = fixture(remote, &server).await;
        enabled_only(&client, &["example"]).await;
        let Output::QueuedTurn(first) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Load the full skill and its resource".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        server.wait_count(3).await;
        assert_eq!(
            history(&client, session.id).await.runs[0].status,
            Status::Running
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        assert_content(&requests[2], &["Complete end", "Guide 1.0.0 中文 🙂"]);
        let receipt = receipt(&requests[2], "call-fixture-0");
        let raw = original(&client, first.id, &receipt).await;
        gate.notify_one();
        let before = finished(&client, session.id, first.id).await;
        assert_eq!(before.runs[0].status, Status::Completed);
        let compact = Server::compaction_text(false, "Summary without skill bodies".into()).await;
        route(&client, &session, &compact).await;
        let Output::QueuedTurn(summary) = execute(
            &client,
            Command::CompactContext {
                focus: None,
                session: session.id,
                expected_revision: session.revision,
            },
        )
        .await
        else {
            panic!("compaction turn expected")
        };
        let compacted = finished(&client, session.id, summary.id).await;
        assert_eq!(compacted.runs.last().unwrap().status, Status::Completed);
        assert_eq!(&compacted.entries[..before.entries.len()], before.entries);
        let requests = compact.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert!(agent_support::compaction::is_summary(&requests[0]));
        assert!(!requests[0].to_string().contains("<turn-context>"));
        observations::assert_context_hidden(&node, session.id).await;
        assert_eq!(
            requests[0]["messages"]
                .to_string()
                .matches("Complete end")
                .count(),
            1
        );
        assert_eq!(
            requests[0]["messages"]
                .to_string()
                .matches("Guide 1.0.0 中文 🙂")
                .count(),
            1
        );
        let continuation = Server::held_completion(Vec::new(), gate.clone()).await;
        route(&client, &session, &continuation).await;
        let Output::QueuedTurn(next) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Continue with the loaded skill".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        continuation.wait_count(1).await;
        assert_eq!(
            history(&client, session.id)
                .await
                .runs
                .last()
                .unwrap()
                .status,
            Status::Running
        );
        let requests = continuation.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_content(&requests[0], &["Complete end", "Guide 1.0.0 中文 🙂"]);
        assert_eq!(owner(&requests[0], "SKILL.md"), raw);
        assert!(
            requests[0]["messages"]
                .to_string()
                .contains("Summary without skill bodies")
        );
        assert!(
            !requests[0]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool")
        );
        assert_eq!(original(&client, next.id, &receipt).await, raw);
        gate.notify_one();
        let after = finished(&client, session.id, next.id).await;
        assert_eq!(after.runs.last().unwrap().status, Status::Completed);
        observations::assert_context_hidden(&node, session.id).await;
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
