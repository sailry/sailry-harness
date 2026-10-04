use super::*;

async fn compact(client: &Client, session: SessionId, revision: u64) -> (Request, QueuedTurn) {
    let request = client.prepare(Command::CompactContext {
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
async fn skips_empty_and_small_context() {
    for remote in [false, true] {
        let server = Server::start(false).await;
        let fixture = Fixture::new(remote, &server).await;
        let (_, empty) = compact(&fixture.client, fixture.session.id, 1).await;
        let page = finished(&fixture.client, fixture.session.id, empty.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.entries.is_empty());
        assert!(server.requests.lock().unwrap().is_empty());
        submit(&fixture.client, fixture.session.id, "Small".into()).await;
        let (_, small) = compact(&fixture.client, fixture.session.id, 1).await;
        let page = finished(&fixture.client, fixture.session.id, small.id).await;
        assert!(page.runs.iter().all(|run| run.status == Status::Completed));
        assert_eq!(summaries(&page), 0);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn supports_unknown_usage() {
    for remote in [false, true] {
        let server = Server::loop_usage(vec![None], 100).await;
        let fixture = Fixture::new(remote, &server).await;
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
            assert!(agent_support::compaction::is_summary(
                requests.last().unwrap()
            ));
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
        let fixture = Fixture::new(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Exact tool evidence 中文 🙂",
        )
        .unwrap();
        let original = "Original manual constraint 中文 🙂 ".repeat(60);
        let (_, first, before) =
            submit(&fixture.client, fixture.session.id, original.clone()).await;
        assert_eq!(summaries(&before), 0);
        let (request, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(summaries(&page), 1);
        assert_eq!(&page.entries[..before.entries.len()], before.entries);
        let entry = page.entries.last().unwrap();
        assert_eq!(entry.turn, turn.id);
        assert_eq!(entry.author, "system");
        assert!(entry.usage.is_some());
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        let summary = &requests[2];
        assert!(agent_support::compaction::is_summary(summary));
        assert!(
            summary["tools"].is_null() || summary["tools"].as_array().is_some_and(Vec::is_empty)
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
        drop(stream);
        let (_, noop) = compact(&fixture.client, fixture.session.id, 1).await;
        let unchanged = finished(&fixture.client, fixture.session.id, noop.id).await;
        assert_eq!(unchanged.entries, page.entries);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
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
        let node = Node::start(fixture.directory.path().join("node"))
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
        assert!(!messages(&current).contains("Original manual constraint"));
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
        let fixture = Fixture::new(remote, &server).await;
        std::fs::write(fixture.root.join("source.txt"), "Keep evidence").unwrap();
        let (_, _, before) = submit(
            &fixture.client,
            fixture.session.id,
            "Keep original requirement ".repeat(80),
        )
        .await;
        let (request, turn) = compact(&fixture.client, fixture.session.id, 1).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.entries, before.entries);
        let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
        assert_eq!(run.status, Status::Failed);
        assert!(run.error.is_some());
        client_retry(&fixture.client, request, turn).await;
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        submit(&fixture.client, fixture.session.id, "Continue".into()).await;
        let current = server.requests.lock().unwrap().last().unwrap().clone();
        assert!(messages(&current).contains("Keep original requirement"));
        assert!(messages(&current).contains("Keep evidence"));
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
        let fixture = Fixture::new(remote, &server).await;
        std::fs::write(fixture.root.join("source.txt"), "Queue evidence").unwrap();
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
                turn: active.id,
                command: "printf 'must not execute'".into(),
                cwd: String::new(),
                timeout_ms: 1000,
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
        let node = Node::start(fixture.directory.path().join("node"))
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
