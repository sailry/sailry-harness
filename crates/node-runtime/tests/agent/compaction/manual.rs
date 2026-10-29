use super::*;
use base64::Engine;
use serde_json::json;

async fn compact(client: &Client, session: SessionId, revision: u64) -> (Request, QueuedTurn) {
    let request = client.prepare(Command::CompactContext {
        focus: None,
        session,
        expected_revision: revision,
    });
    let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
        panic!("turn expected")
    };
    assert_eq!(turn.kind, RunKind::Compaction);
    (request, turn)
}

fn summaries(page: &Page) -> usize {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter(|part| matches!(part, Part::Compaction(_)))
        .count()
}

fn prompt(request: &Value) -> &str {
    assert!(agent_support::compaction::is_summary(request));
    assert!(
        request
            .get("tools")
            .is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty))
    );
    assert!(request.get("tool_choice").is_none());
    let messages = request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(
        messages[1]["content"],
        "Please summarize the conversation history provided in the system prompt."
    );
    messages[0]["content"].as_str().unwrap()
}

#[tokio::test]
async fn loads_captured_plugin_context() {
    for remote in [false, true] {
        let server = Server::summary(100).await;
        let fixture = Fixture::packages(remote, &server, &["files", "goals"]).await;
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPlugin {
                name: "goals".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("goals package expected")
        };
        if !package.summary.enabled {
            fixture
                .client
                .execute(fixture.client.prepare(Command::SetPluginEnabled {
                    name: "goals".into(),
                    expected_revision: package.summary.revision,
                    enabled: true,
                }))
                .await
                .unwrap();
        }
        std::fs::write(
            fixture.root.join("source.txt"),
            "Retained file evidence\n".repeat(200),
        )
        .unwrap();
        let (_, _, before) = submit(
            &fixture.client,
            fixture.session.id,
            "Read the evidence before the handoff".into(),
        )
        .await;
        let (_, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        assert!(turn.plugins.iter().any(|package| package.name == "goals"));
        let after = finished(&fixture.client, fixture.session.id, turn.id).await;
        let run = after.runs.iter().find(|run| run.turn == turn.id).unwrap();
        assert_eq!(run.status, Status::Completed, "{run:?}");
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        assert_eq!(summaries(&after), 1);
        {
            let requests = server.requests.lock().unwrap();
            let summary = requests.last().unwrap();
            prompt(summary);
        }
        let (_, _, continued) = submit(
            &fixture.client,
            fixture.session.id,
            "Continue with the retained evidence".into(),
        )
        .await;
        assert_eq!(summaries(&continued), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn formats_materialized_attachments() {
    let text = format!(
        "Inline payload start\n{}\nInline payload end",
        "Materialized attachment evidence. ".repeat(12_000)
    );
    let mut document = b"%PDF-1.7\n".to_vec();
    document.resize(504 * 1024, b' ');
    for remote in [false, true] {
        for (name, media_type, context, summary_context, output, payload) in [
            (
                "evidence.txt",
                "text/plain",
                512_000,
                32_000,
                2_048,
                text.as_bytes(),
            ),
            (
                "evidence.pdf",
                "application/pdf",
                200_000,
                64_000,
                32_000,
                document.as_slice(),
            ),
        ] {
            let server = Server::summary(100).await;
            let mut fixture = fixture(remote, &server).await;
            if media_type == "application/pdf" {
                // Azure shares this Chat Completions fixture and explicitly
                // supports native PDFs, unlike arbitrary compatible endpoints.
                let mut provider =
                    configuration::snapshot(&fixture.client).await.providers[0].clone();
                provider.api = sailry_protocol::conversation::ModelApi::AzureOpenAi;
                provider.options = Some(
                    sailry_protocol::conversation::connection::Options::AzureOpenAi {
                        api_version: "2024-12-01-preview".into(),
                    },
                );
                let Output::Provider(provider) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::SaveProvider {
                        expected_revision: provider.revision,
                        provider,
                        secret: Some(sailry_protocol::Secret::new("fixture-cloud-key".into())),
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("provider expected")
                };
                let mut config = fixture.session.config.clone();
                config.credential = provider.credential;
                let Output::Session(session) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::SetSessionConfig {
                        session: fixture.session.id,
                        expected_revision: fixture.session.revision,
                        config,
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("session expected")
                };
                fixture.session = session;
            }
            super::budget::model(&fixture, context, output, true).await;
            std::fs::write(
                fixture.root.join("source.txt"),
                "Historical file evidence\n".repeat(200),
            )
            .unwrap();
            let attachment = super::budget::upload(&fixture, name, media_type, payload).await;
            let Output::QueuedTurn(original) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: Input {
                        text: "Keep this attachment and the file evidence".into(),
                        attachments: vec![attachment],
                        ..Default::default()
                    },
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let before = finished(&fixture.client, fixture.session.id, original.id).await;
            assert_eq!(
                before.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                before.runs
            );
            let expected = if media_type == "application/pdf" {
                format!(
                    "data:application/pdf;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(payload)
                )
            } else {
                "Inline payload start".into()
            };
            assert!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|request| messages(request).contains(&expected))
            );
            super::budget::model(&fixture, summary_context, output, true).await;
            let (_, turn) = compact(
                &fixture.client,
                fixture.session.id,
                fixture.session.revision,
            )
            .await;
            let after = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                after.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                after.runs
            );
            assert_eq!(&after.entries[..before.entries.len()], before.entries);
            assert_eq!(summaries(&after), 1);
            {
                let requests = server.requests.lock().unwrap();
                let summary = requests.last().unwrap();
                let text = prompt(summary);
                if media_type == "application/pdf" {
                    assert!(text.contains("[document: evidence.pdf (application/pdf)]"));
                    assert!(!text.contains("data:application/pdf;base64,"));
                } else {
                    assert!(text.contains("Inline payload start"));
                    assert!(text.contains("Inline payload end"));
                }
                assert!(!text.contains("sailry-attachment://"));
            }
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "Continue after the attachment handoff".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let continued = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(continued.runs.last().unwrap().status, Status::Completed);
            assert_eq!(summaries(&continued), 1);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn formats_historical_attachments() {
    for remote in [false, true] {
        let server = Server::summary(100).await;
        let fixture = fixture(remote, &server).await;
        super::budget::model(&fixture, 64_000, 2_048, true).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Attachment evidence\n".repeat(200),
        )
        .unwrap();
        let attachment = super::budget::image(&fixture).await;
        let Output::QueuedTurn(original) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: Input {
                    text: "Preserve this image and the file evidence".into(),
                    attachments: vec![attachment],
                    ..Default::default()
                },
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let before = finished(&fixture.client, fixture.session.id, original.id).await;
        assert_eq!(
            before.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            before.runs
        );
        let (_, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let after = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            after.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            after.runs
        );
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        assert_eq!(summaries(&after), 1);
        {
            let requests = server.requests.lock().unwrap();
            let summary = requests.last().unwrap();
            assert!(prompt(summary).contains("[image: image/png]"));
            assert!(!messages(summary).contains("data:image/png;base64,"));
            assert!(!messages(summary).contains("sailry-attachment://"));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn handles_empty_and_small_context() {
    for remote in [false, true] {
        let server = Server::summary(100).await;
        let fixture = fixture(remote, &server).await;
        let (_, empty) = compact(&fixture.client, fixture.session.id, 1).await;
        let page = finished(&fixture.client, fixture.session.id, empty.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(summaries(&page), 1);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            prompt(&requests[0]);
        }
        let (_, _, before) = submit(&fixture.client, fixture.session.id, "Small".into()).await;
        let (_, small) = compact(&fixture.client, fixture.session.id, 1).await;
        let compacted = finished(&fixture.client, fixture.session.id, small.id).await;
        assert!(
            compacted
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        assert_eq!(&compacted.entries[..before.entries.len()], before.entries);
        assert_eq!(summaries(&compacted), 2);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        let (_, repeated) = compact(&fixture.client, fixture.session.id, 1).await;
        let repeated = finished(&fixture.client, fixture.session.id, repeated.id).await;
        assert_eq!(
            &repeated.entries[..compacted.entries.len()],
            compacted.entries
        );
        assert_eq!(summaries(&repeated), 3);
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn accepts_a_short_two_turn_handoff() {
    for remote in [false, true] {
        let server = Server::compaction(false).await;
        let fixture = fixture(remote, &server).await;
        super::budget::model(&fixture, 500_000, 128_000, false).await;
        let sessions = (0..7)
            .map(|index| {
                json!({
                    "id":format!("retained-session-{index}"),
                    "title":format!("Saved session {index}"),
                    "detail":"Retained metadata only. ".repeat(80)
                })
            })
            .collect::<Vec<_>>();
        std::fs::write(
            fixture.root.join("source.txt"),
            serde_json::to_vec(&json!({"sessions":sessions})).unwrap(),
        )
        .unwrap();
        submit(
            &fixture.client,
            fixture.session.id,
            "List saved sessions".into(),
        )
        .await;
        let (_, _, before) = submit(
            &fixture.client,
            fixture.session.id,
            "Retain those session identities for follow-up work".into(),
        )
        .await;
        let count = server.requests.lock().unwrap().len();
        let (request, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let after = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(after.runs.last().unwrap().status, Status::Completed);
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        assert_eq!(summaries(&after), 1);
        assert!(
            !after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, Part::Resource(value) if value["type"] == "compaction_failure")
                })
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), count + 1);
            let summary = requests.last().unwrap();
            assert!(prompt(summary).contains("Retain those session identities"));
            let output = summary["max_completion_tokens"]
                .as_u64()
                .or_else(|| summary["max_tokens"].as_u64())
                .unwrap();
            assert_eq!(output, 128_000);
        }
        fixture.client.execute(request).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), count + 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_verbatim_only_history() {
    for remote in [false, true] {
        let server = Server::summary(100).await;
        let fixture = fixture(remote, &server).await;
        let original = "Retain exact constraints 中文 🙂 ".repeat(200);
        let (_, _, before) = submit(&fixture.client, fixture.session.id, original.clone()).await;
        assert!(original.len() > 1_024);
        let count = server.requests.lock().unwrap().len();
        let (_, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let after = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(after.runs.last().unwrap().status, Status::Completed);
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        assert_eq!(summaries(&after), 1);
        assert_eq!(server.requests.lock().unwrap().len(), count + 1);
        assert!(
            after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| { matches!(part, Part::Text(text) if text == &original) })
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn supports_unknown_usage() {
    for remote in [false, true] {
        let server = Server::loop_usage(vec![None], 100).await;
        let fixture = fixture(remote, &server).await;
        // Unknown usage still permits explicit compaction of the complete history.
        super::budget::model(&fixture, 128_000, 2_048, false).await;
        for index in 1..=6 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Unmetered evidence {index}\n").repeat(1000),
            )
            .unwrap();
        }
        let (_, _, before) = submit(
            &fixture.client,
            fixture.session.id,
            "Read all six files".into(),
        )
        .await;
        assert_eq!(summaries(&before), 0);
        assert_eq!(server.requests.lock().unwrap().len(), 7);
        let (_, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let after = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(after.runs.last().unwrap().status, Status::Completed);
        assert_eq!(summaries(&after), 1);
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 8);
            let summary = requests.last().unwrap();
            let transcript = prompt(summary);
            assert!(transcript.contains("Unmetered evidence 1"));
            assert!(transcript.contains("Unmetered evidence 6"));
        }
        usage(&fixture.client, 1).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn restores_branch_history() {
    for remote in [false, true] {
        let server = Server::compaction(false).await;
        let fixture = fixture(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Exact tool evidence 中文 🙂\n".repeat(200),
        )
        .unwrap();
        let original = "Original manual constraint 中文 🙂 ".repeat(60);
        let (_, first, before) =
            submit(&fixture.client, fixture.session.id, original.clone()).await;
        assert_eq!(summaries(&before), 0);
        let outputs = results(&fixture.client, &before).await;
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0]["kind"], "file_lines");
        let reference = sailry_protocol::tool::OutputReference {
            call: "evidence-call".into(),
            revision: blake3::hash(outputs[0].to_string().as_bytes())
                .to_hex()
                .to_string(),
        };
        assert!(
            before
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, Part::ToolCall { id: Some(id), name, .. }
                if id == &reference.call && name == &plugin_tool("files", "read_file"))
                })
        );
        let measured = super::budget::statistics(&fixture).await;
        let (request, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(summaries(&page), 1);
        assert_eq!(&page.entries[..before.entries.len()], before.entries);
        let entry = page.entries.last().unwrap();
        assert_eq!(entry.turn, turn.id);
        assert_eq!(entry.author, "system");
        // Summary usage is billed to a hidden compaction event; statistics follow.
        assert!(entry.usage.is_none());
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        let summary = &requests[2];
        let transcript = prompt(summary);
        assert!(transcript.contains("Exact tool evidence 中文 🙂"));
        assert!(transcript.contains(&plugin_tool("files", "read_file")));
        // Source references are transcript data, not replayed tool-call fields.
        let references = transcript
            .lines()
            .filter(|line| line.contains("evidence-call"))
            .collect::<Vec<_>>();
        assert_eq!(references.len(), 1);
        let footer: Value = serde_json::from_str(references[0]).unwrap();
        assert_eq!(footer, json!({"output":reference,"next_offset":null}));
        let mut stream = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let Update::ConversationSnapshot(snapshot) = stream.next().await.unwrap() else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.statistics.turns, 1);
        assert_eq!(snapshot.statistics.responses, 3);
        assert_eq!(snapshot.statistics.context_tokens, None);
        assert!(
            snapshot
                .statistics
                .context_estimate
                .is_some_and(|tokens| tokens > 0)
        );
        assert!(
            snapshot
                .statistics
                .context_limit
                .is_some_and(|limit| limit > 0)
        );
        assert!(
            snapshot.statistics.usage.as_ref().unwrap().input
                >= measured.usage.as_ref().unwrap().input
        );
        assert!(
            snapshot.statistics.usage.as_ref().unwrap().output
                >= measured.usage.as_ref().unwrap().output
        );
        drop(stream);
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            Output::QueuedTurn(turn.clone())
        );
        let unchanged = history(&fixture.client, fixture.session.id).await;
        assert_eq!(unchanged.entries, page.entries);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        let estimated = super::budget::statistics(&fixture).await;
        let mut branches = Vec::new();
        for through in [first, turn.id] {
            let Output::Session(branch) = fixture
                .client
                .execute(fixture.client.prepare(Command::ForkConversation {
                    session: fixture.session.id,
                    through,
                    expected_revision: 1,
                }))
                .await
                .unwrap()
            else {
                panic!("fork expected")
            };
            branches.push(branch);
        }
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(fixture.directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, fixture.session.id).await, unchanged);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        usage(&client, 3).await;
        let mut updates = client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let Update::ConversationSnapshot(reopened) = updates.next().await.unwrap() else {
            panic!("snapshot expected")
        };
        assert_eq!(reopened.statistics, estimated);
        drop(updates);
        // A new summary on an early branch belongs to that branch's maintenance turn.
        let (_, branch_turn) = compact(&client, branches[0].id, 1).await;
        let branch_page = finished(&client, branches[0].id, branch_turn.id).await;
        assert_eq!(summaries(&branch_page), 1);
        assert_eq!(branch_page.entries.last().unwrap().turn, branch_turn.id);
        assert_eq!(history(&client, fixture.session.id).await, unchanged);
        let (_, inherited) = compact(&client, branches[1].id, 1).await;
        let inherited_page = finished(&client, branches[1].id, inherited.id).await;
        assert_eq!(summaries(&inherited_page), 2);
        assert_eq!(server.requests.lock().unwrap().len(), 5);
        submit(&client, fixture.session.id, "Continue from summary".into()).await;
        let current = server.requests.lock().unwrap().last().unwrap().clone();
        assert!(messages(&current).contains("Context summary fixture"));
        assert!(!messages(&current).contains("Original manual constraint"));
        assert!(!messages(&current).contains("evidence-call"));
        assert!(
            current["messages"]
                .as_array()
                .unwrap()
                .iter()
                .all(|message| {
                    message["role"] != "tool"
                        && message.get("tool_calls").is_none()
                        && message.get("tool_call_id").is_none()
                })
        );
        assert_eq!(server.requests.lock().unwrap().len(), 6);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_failed_context() {
    for remote in [false, true] {
        let server = Server::compaction(true).await;
        let fixture = fixture(remote, &server).await;
        std::fs::write(fixture.root.join("source.txt"), "Keep evidence").unwrap();
        let (_, _, before) = submit(
            &fixture.client,
            fixture.session.id,
            "Keep original requirement ".repeat(80),
        )
        .await;
        let original_requests = server.requests.lock().unwrap().len();
        assert_eq!(original_requests, 2);
        let (request, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains(agent_support::compaction::PRIVATE_SENTINEL)
        );
        assert_eq!(&page.entries[..before.entries.len()], before.entries);
        assert_eq!(page.entries.len(), before.entries.len() + 1);
        let failure = page.entries.last().unwrap();
        assert_eq!(failure.turn, turn.id);
        assert_eq!(failure.author, "assistant");
        assert_eq!(summaries(&page), 0);
        let snapshot = super::super::configuration::snapshot(&fixture.client).await;
        assert!(
            !serde_json::to_string(&snapshot.notifications)
                .unwrap()
                .contains(agent_support::compaction::PRIVATE_SENTINEL)
        );
        let notices: Vec<_> = snapshot.notifications.iter().filter(|notice| {
            matches!(&notice.content.details, Some(sailry_protocol::notification::Details::Task(task)) if task.turn == turn.id)
        }).collect();
        assert_eq!(notices.len(), 1);
        let Some(sailry_protocol::notification::Details::Task(task)) = &notices[0].content.details
        else {
            unreachable!()
        };
        assert_eq!(task.status, Status::Failed);
        assert_eq!(notices[0].content.session, Some(fixture.session.id));
        let mut stream = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let Update::ConversationSnapshot(snapshot) = stream.next().await.unwrap() else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.page.entries, page.entries);
        drop(stream);
        let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
        assert_eq!(run.status, Status::Failed);
        assert_eq!(run.error.as_ref().unwrap().code, ErrorCode::Unavailable);
        assert_eq!(
            run.error.as_ref().unwrap().details,
            Some(FaultDetails::ModelRequest {
                reason: ModelFailure::Request
            })
        );
        assert!(
            run.error
                .as_ref()
                .unwrap()
                .message
                .starts_with("Ran into this error trying to compact:")
        );
        assert!(
            run.error
                .as_ref()
                .unwrap()
                .message
                .contains("Summary service unavailable")
        );
        let summary_attempts = goose_provider_types::retry::RetryConfig::default().max_retries + 1;
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), original_requests + summary_attempts);
            for request in &requests[original_requests..] {
                let transcript = prompt(request);
                assert!(transcript.contains("Keep original requirement"));
                assert!(transcript.contains("Keep evidence"));
            }
        }
        client_retry(&fixture.client, request, turn).await;
        assert_eq!(
            server.requests.lock().unwrap().len(),
            original_requests + summary_attempts
        );
        submit(&fixture.client, fixture.session.id, "Continue".into()).await;
        let current = server.requests.lock().unwrap().last().unwrap().clone();
        assert!(messages(&current).contains("Keep original requirement"));
        assert!(messages(&current).contains("Keep evidence"));
        assert!(!messages(&current).contains("compaction_failure"));
        assert!(!messages(&current).contains(agent_support::compaction::PRIVATE_SENTINEL));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

async fn client_retry(client: &Client, request: Request, turn: QueuedTurn) {
    assert_eq!(
        client.execute(request).await.unwrap(),
        Output::QueuedTurn(turn)
    );
}

#[tokio::test]
async fn preserves_cancelled_configuration() {
    for remote in [false, true] {
        let server = Server::compaction(false).await;
        let slow = Server::start(true).await;
        let fixture = fixture(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Queue evidence\n".repeat(200),
        )
        .unwrap();
        let (_, _, before) = submit(
            &fixture.client,
            fixture.session.id,
            "Frozen requirement ".repeat(80),
        )
        .await;
        let snapshot = configuration::snapshot(&fixture.client).await;
        let mut provider = snapshot.providers[0].clone();
        provider.endpoint = slow.endpoint.clone();
        let Output::Provider(mut provider) = fixture
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
        let (cancel_request, active) = compact(&fixture.client, fixture.session.id, 1).await;
        slow.wait_count(1).await;
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::RunCommand {
                access: sailry_protocol::process::Access::Workspace,
                description: None,
                turn: active.id,
                command: "printf 'must not execute'".into(),
                cwd: String::new(),
                timeout_ms: 1000,
                pty: false,
                background: false,
                attachments: vec![],
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        provider.endpoint = server.endpoint.clone();
        let Output::Provider(mut provider) = fixture
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
        let (_, removed) = compact(&fixture.client, fixture.session.id, 1).await;
        let (_, queued) = compact(&fixture.client, fixture.session.id, 1).await;
        let queued_message = queue::read(&fixture.client, queued.id).await;
        assert_eq!(queued_message.turn, queued);
        assert_eq!(queued_message.message, Input::default());
        let current = history(&fixture.client, fixture.session.id).await.queue;
        assert_eq!(current.items.len(), 2);
        assert!(
            current
                .items
                .iter()
                .all(|item| item.kind == RunKind::Compaction && item.preview.is_empty())
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetQueuePaused {
                session: fixture.session.id,
                expected_revision: current.revision,
                paused: true,
            }))
            .await
            .unwrap();
        let current = history(&fixture.client, fixture.session.id).await.queue;
        fixture
            .client
            .execute(fixture.client.prepare(Command::MoveQueuedTurn {
                session: fixture.session.id,
                expected_revision: current.revision,
                turn: queued.id,
                before: Some(removed.id),
            }))
            .await
            .unwrap();
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::EditQueuedTurn {
                turn: queued.id,
                expected_revision: 1,
                message: "must not become a task".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        fixture
            .client
            .execute(fixture.client.prepare(Command::RemoveQueuedTurn {
                turn: removed.id,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let mut config = fixture.session.config.clone();
        config.effort = Effort::Low;
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: 1,
                config,
            }))
            .await
            .unwrap();
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::CompactContext {
                focus: None,
                session: fixture.session.id,
                expected_revision: 1,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        provider.endpoint = "http://127.0.0.1:1/v1".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: active.id }),
            )
            .await
            .unwrap();
        let cancelled = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(cancelled.entries, before.entries);
        assert_eq!(
            cancelled
                .runs
                .iter()
                .find(|run| run.turn == active.id)
                .unwrap()
                .status,
            Status::Cancelled
        );
        assert!(cancelled.queue.paused);
        assert_eq!(cancelled.queue.items[0].turn, queued.id);
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(fixture.directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(queue::read(&client, queued.id).await, queued_message);
        client_retry(&client, cancel_request, active).await;
        assert_eq!(slow.requests.lock().unwrap().len(), 1);
        client
            .execute(client.prepare(Command::SendQueuedTurn {
                turn: queued.id,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let page = finished(&client, fixture.session.id, queued.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == queued.id)
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(summaries(&page), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        assert_eq!(
            server.requests.lock().unwrap().last().unwrap()["model"],
            "fixture-a"
        );
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn accepts_larger_raw_handoffs() {
    for remote in [false, true] {
        let handoff = "Verified complete handoff. ".repeat(2_000);
        let server = Server::compaction_text(false, handoff.clone()).await;
        let fixture = fixture(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Exact synthetic source evidence.\n".repeat(200),
        )
        .unwrap();
        let (_, _, before) = submit(
            &fixture.client,
            fixture.session.id,
            "Inspect the source".into(),
        )
        .await;
        let (_, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let after = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(after.runs.last().unwrap().status, Status::Completed);
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        assert_eq!(summaries(&after), 1);
        let summary = after
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .find_map(|part| match part {
                Part::Compaction(text) => Some(text),
                _ => None,
            })
            .unwrap();
        assert_eq!(summary, &handoff);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            let transcript = prompt(&requests[2]);
            assert!(handoff.len() > transcript.len());
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
