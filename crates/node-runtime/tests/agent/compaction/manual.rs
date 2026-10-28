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

#[tokio::test]
async fn bounds_materialized_attachments() {
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
                assert!(agent_support::compaction::is_summary(summary));
                assert_eq!(summary["tools"], requests[requests.len() - 2]["tools"]);
                assert_eq!(summary["tool_choice"], "none");
                let text = messages(summary);
                assert!(text.contains("Contents not included because they exceed"));
                assert!(text.contains(&attachment.to_string()));
                assert!(!text.contains("Inline payload start"));
                assert!(!text.contains("Inline payload end"));
                assert!(!text.contains("data:application/pdf;base64,"));
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
async fn hydrates_historical_attachments() {
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
            assert!(agent_support::compaction::is_summary(summary));
            assert_eq!(summary["tools"], requests[requests.len() - 2]["tools"]);
            assert_eq!(summary["tool_choice"], "none");
            assert!(messages(summary).contains("data:image/png;base64,"));
            assert!(!messages(summary).contains("sailry-attachment://"));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn skips_empty_and_summarizes_small_context() {
    for remote in [false, true] {
        let server = Server::summary(100).await;
        let fixture = fixture(remote, &server).await;
        let (_, empty) = compact(&fixture.client, fixture.session.id, 1).await;
        let page = finished(&fixture.client, fixture.session.id, empty.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.entries.is_empty());
        assert!(server.requests.lock().unwrap().is_empty());
        let (_, _, before) = submit(&fixture.client, fixture.session.id, "Small".into()).await;
        // Observation metadata does not replace the completed interaction boundary.
        let (_, small) = compact(&fixture.client, fixture.session.id, 1).await;
        let compacted = finished(&fixture.client, fixture.session.id, small.id).await;
        assert!(
            compacted
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        assert_eq!(&compacted.entries[..before.entries.len()], before.entries);
        assert_eq!(summaries(&compacted), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        // A summary without later work has no additional eligible history.
        let (_, noop) = compact(&fixture.client, fixture.session.id, 1).await;
        let unchanged = finished(&fixture.client, fixture.session.id, noop.id).await;
        assert_eq!(unchanged.entries, compacted.entries);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
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
            assert!(agent_support::compaction::is_summary(summary));
            let prompt = summary["messages"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|message| message["role"] == "user")
                .unwrap()["content"]
                .as_str()
                .unwrap();
            assert!(prompt.contains("Summary output budget: 128000 tokens"));
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
            assert!(agent_support::compaction::is_summary(summary));
            assert!(messages(summary).contains("Unmetered evidence 1"));
            assert!(messages(summary).contains("Unmetered evidence 6"));
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
        assert!(agent_support::compaction::is_summary(summary));
        assert_eq!(summary["tools"], requests[1]["tools"]);
        let normal = requests[1]["messages"].as_array().unwrap();
        assert_eq!(
            summary["messages"].as_array().unwrap()[..normal.len()],
            normal[..]
        );
        assert!(messages(summary).contains("Exact tool evidence 中文 🙂"));
        assert!(messages(summary).contains("evidence-call"));
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
        let (_, noop) = compact(&fixture.client, fixture.session.id, 1).await;
        let unchanged = finished(&fixture.client, fixture.session.id, noop.id).await;
        assert_eq!(unchanged.entries, page.entries);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        // The maintenance turn contributes its own timing before completion.
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
        assert_eq!(summaries(&inherited_page), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        submit(&client, fixture.session.id, "Continue from summary".into()).await;
        let current = server.requests.lock().unwrap().last().unwrap().clone();
        assert!(messages(&current).contains("Context summary fixture"));
        assert!(messages(&current).contains("Original manual constraint"));
        assert!(!messages(&current).contains("evidence-call"));
        assert_eq!(server.requests.lock().unwrap().len(), 5);
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
        let (request, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains(agent_support::compaction::PRIVATE_SENTINEL)
        );
        assert_eq!(&page.entries[..before.entries.len()], before.entries);
        let failure = page.entries.last().unwrap();
        assert_eq!(failure.turn, turn.id);
        let Part::Resource(value) = &failure.parts[0] else {
            panic!("failure expected")
        };
        assert_eq!(value["type"], "compaction_failure");
        assert_eq!(value["failure"]["source"], "manual");
        assert_eq!(value["failure"]["stage"], "request");
        assert_eq!(value["failure"]["reason"], "service_rejected");
        assert!(
            value["failure"]["cause"]
                .as_str()
                .unwrap()
                .contains("Summary service unavailable")
        );
        assert!(
            !value["failure"]["cause"]
                .as_str()
                .unwrap()
                .contains(agent_support::compaction::PRIVATE_SENTINEL)
        );
        assert_eq!(value["failure"]["continued"], false);
        let snapshot = super::super::configuration::snapshot(&fixture.client).await;
        assert!(
            !serde_json::to_string(&snapshot.notifications)
                .unwrap()
                .contains(agent_support::compaction::PRIVATE_SENTINEL)
        );
        let notices: Vec<_> = snapshot.notifications.iter().filter(|notice| {
            matches!(&notice.content.details, Some(sailry_protocol::notification::Details::Diagnostic(details)) if details.turn == turn.id)
        }).collect();
        assert_eq!(notices.len(), 1);
        let Some(sailry_protocol::notification::Details::Diagnostic(details)) =
            &notices[0].content.details
        else {
            unreachable!()
        };
        assert_eq!(details.event, failure.id);
        assert_eq!(details.sequence, failure.sequence);
        assert_eq!(notices[0].content.session, Some(fixture.session.id));
        assert!(!snapshot.notifications.iter().any(|notice| matches!(&notice.content.details, Some(sailry_protocol::notification::Details::Task(task)) if task.turn == turn.id)));
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
        assert!(run.error.is_some());
        client_retry(&fixture.client, request, turn).await;
        assert_eq!(server.requests.lock().unwrap().len(), 3);
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
async fn rejects_unhelpful_covered_input() {
    for remote in [false, true] {
        let server = Server::summary(100).await;
        let fixture = fixture(remote, &server).await;
        super::budget::model(&fixture, 128_000, 2_048, false).await;
        let original = "x".repeat(31 * 1024);
        let (_, _, before) = submit(&fixture.client, fixture.session.id, original.clone()).await;
        let (_, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let after = finished(&fixture.client, fixture.session.id, turn.id).await;
        let run = after.runs.iter().find(|run| run.turn == turn.id).unwrap();
        assert_eq!(run.status, Status::Failed, "{run:?}");
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        assert_eq!(summaries(&after), 0);
        assert!(
            after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, Part::Resource(value) if value["type"] == "compaction_failure" && value["failure"]["reason"]=="no_reduction")
                })
        );
        assert!(
            after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Text(text) if text == &original))
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(agent_support::compaction::is_summary(&requests[1]));
            assert!(messages(&requests[1]).contains(&original));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_larger_complete_handoffs() {
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
        assert_eq!(after.runs.last().unwrap().status, Status::Failed);
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        assert_eq!(summaries(&after), 0);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert!(agent_support::compaction::is_summary(&requests[2]));
            let transcript = requests[2]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "user")
                .unwrap()["content"]
                .as_str()
                .unwrap();
            assert!(handoff.len() > transcript.len());
            assert!(
            after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, Part::Resource(value) if value["type"] == "compaction_failure" && value["failure"]["reason"]=="no_reduction")
                })
        );
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
