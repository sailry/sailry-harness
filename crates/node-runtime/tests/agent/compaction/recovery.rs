use super::*;

#[tokio::test]
async fn retries_transient_summary_without_replaying_tools() {
    for remote in [false, true] {
        let server = Server::recovering_summary().await;
        let fixture = fixture(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Verified tool evidence\n".repeat(200),
        )
        .unwrap();
        let (_, _, before) = submit(
            &fixture.client,
            fixture.session.id,
            "Read source.txt".into(),
        )
        .await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::CompactContext {
                focus: None,
                session: fixture.session.id,
                expected_revision: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected");
        };
        let after = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            after
                .runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(
            requests
                .iter()
                .filter(|request| agent_support::compaction::is_summary(request))
                .count(),
            2
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| !agent_support::compaction::is_summary(request))
                .count(),
            2
        );
        assert_eq!(
            after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::Compaction(_)))
                .count(),
            1
        );
        assert_eq!(
            after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolResult { .. }))
                .count(),
            1
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn continues_after_unavailable_summary() {
    for remote in [false, true] {
        let server = Server::unavailable_summaries().await;
        let fixture = fixture(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Retained source evidence\n".repeat(200),
        )
        .unwrap();
        submit(
            &fixture.client,
            fixture.session.id,
            "Read source.txt".into(),
        )
        .await;
        let message = "Continue with the original evidence";
        let (_, turn, page) = submit(&fixture.client, fixture.session.id, message.into()).await;
        assert!(!page.queue.paused);
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry.turn == turn && entry.author == "user")
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::Text(text) if text == message))
                .count(),
            1
        );
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolResult { .. }))
                .count(),
            1
        );
        assert!(
            !page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Compaction(_)))
        );
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
            matches!(part, Part::Resource(value) if value["type"] == "compaction_failure"
                && value["failure"]["reason"] == "service_rejected" && value["failure"]["continued"] == true)
        }));
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(
            requests
                .iter()
                .filter(|request| agent_support::compaction::is_summary(request))
                .count(),
            2
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| !agent_support::compaction::is_summary(request))
                .count(),
            3
        );
        assert!(messages(requests.last().unwrap()).contains("Retained source evidence"));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn halts_without_valid_recovery() {
    for remote in [false, true] {
        for (text, reason) in [
            (String::new(), "empty_summary"),
            ("Expanded handoff evidence ".repeat(400), "no_reduction"),
        ] {
            let server = Server::compaction_sequence(false, text, vec![100, 26_000, 100]).await;
            let fixture = fixture(remote, &server).await;
            std::fs::write(
                fixture.root.join("source.txt"),
                "Verified tool evidence\n".repeat(80),
            )
            .unwrap();
            let (_, _, before) = submit(
                &fixture.client,
                fixture.session.id,
                "Read source.txt".into(),
            )
            .await;
            let message = "Continue with verified evidence";
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: 1,
                    message: message.into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected");
            };
            let after = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                after
                    .runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Failed
            );
            assert_eq!(&after.entries[..before.entries.len()], before.entries);
            assert!(after.queue.paused);
            assert_eq!(after.queue.items.len(), 1);
            assert_eq!(after.queue.items[0].turn, turn.id);
            assert!(!after.queue.items[0].ready);
            assert!(
                !after
                    .entries
                    .iter()
                    .any(|entry| entry.turn == turn.id && entry.author == "user")
            );
            let Output::QueuedMessage(retained) = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::ReadQueuedTurn { turn: turn.id }),
                )
                .await
                .unwrap()
            else {
                panic!("retained input expected");
            };
            assert_eq!(retained.message.text, message);
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| agent_support::compaction::is_summary(request))
                    .count(),
                1
            );
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| !agent_support::compaction::is_summary(request))
                    .count(),
                2
            );
            assert_eq!(requests.len(), 3);
            assert!(agent_support::compaction::is_summary(&requests[2]));
            assert!(
                !after
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part, Part::Compaction(_)))
            );
            assert!(
            after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, Part::Resource(value) if value["type"] == "compaction_failure"
                && value["failure"]["source"] == "automatic"
                && value["failure"]["stage"] == "summary"
                && value["failure"]["reason"] == reason
                && value["failure"]["continued"] == false)
                })
        );
            let count: i64 = fixture.node.with_storage(|db| Box::pin(async move {
            db.prepare("SELECT count(*) FROM agent_events WHERE json_type(body,'$.actions.compaction')='object'")
                .await.map_err(crate::database_fixture::fault)?
                .query_row(()).await.map_err(crate::database_fixture::fault)?
                .get(0).map_err(crate::database_fixture::fault)
        })).await.unwrap();
            assert_eq!(count, 0);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn rejects_truncation_and_failed_saves_without_advancing() {
    for remote in [false, true] {
        for storage_failure in [false, true] {
            let server = if storage_failure {
                Server::compaction(false).await
            } else {
                Server::truncated_summary().await
            };
            let fixture = fixture(remote, &server).await;
            std::fs::write(
                fixture.root.join("source.txt"),
                "Verified source evidence\n".repeat(200),
            )
            .unwrap();
            let (_, _, before) = submit(
                &fixture.client,
                fixture.session.id,
                "Read source.txt".into(),
            )
            .await;
            if storage_failure {
                fixture.node.with_storage(|db| Box::pin(async move {
                    db.execute_batch("CREATE TRIGGER reject_summary BEFORE INSERT ON agent_events WHEN json_type(NEW.body,'$.actions.compaction')='object' BEGIN SELECT RAISE(ABORT,'PRIVATE_SAVE_FAILURE_SENTINEL'); END;").await.map_err(crate::database_fixture::fault)
                })).await.unwrap();
            }
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::CompactContext {
                    focus: None,
                    session: fixture.session.id,
                    expected_revision: 1,
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected");
            };
            let after = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                after
                    .runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Failed
            );
            assert_eq!(&after.entries[..before.entries.len()], before.entries);
            assert!(
                !after
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part, Part::Compaction(_)))
            );
            let reason = if storage_failure {
                "persistence_failed"
            } else {
                "truncated_summary"
            };
            assert!(after.entries.iter().flat_map(|entry| &entry.parts).any(
                |part| matches!(part, Part::Resource(value) if value["failure"]["reason"] == reason)
            ));
            assert!(
                !serde_json::to_string(&after)
                    .unwrap()
                    .contains("PRIVATE_SAVE_FAILURE_SENTINEL")
            );
            let count: i64 = fixture.node.with_storage(|db| Box::pin(async move {
                db.prepare("SELECT count(*) FROM agent_events WHERE json_type(body,'$.actions.compaction')='object'")
                    .await.map_err(crate::database_fixture::fault)?
                    .query_row(()).await.map_err(crate::database_fixture::fault)?
                    .get(0).map_err(crate::database_fixture::fault)
            })).await.unwrap();
            assert_eq!(count, 0);
            if storage_failure {
                fixture
                    .node
                    .with_storage(|db| {
                        Box::pin(async move {
                            db.execute_batch("DROP TRIGGER reject_summary")
                                .await
                                .map_err(crate::database_fixture::fault)
                        })
                    })
                    .await
                    .unwrap();
            }
            submit(
                &fixture.client,
                fixture.session.id,
                "Continue with the original evidence".into(),
            )
            .await;
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| agent_support::compaction::is_summary(request))
                    .count(),
                1
            );
            let resumed = messages(requests.last().unwrap());
            assert!(resumed.contains("evidence-call"));
            assert!(resumed.contains("Verified source evidence"));
            assert!(!resumed.contains("compaction_failure"));
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| !agent_support::compaction::is_summary(request))
                    .count(),
                3
            );
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn compacts_after_provider_overflow() {
    for remote in [false, true] {
        recover_before_output(remote, false).await;
    }
}

#[tokio::test]
async fn compacts_after_early_length() {
    for remote in [false, true] {
        recover_before_output(remote, true).await;
    }
}

async fn recover_before_output(remote: bool, early: bool) {
    let server = if early {
        Server::early_length().await
    } else {
        Server::context_overflow().await
    };
    let fixture = fixture(remote, &server).await;
    std::fs::write(
        fixture.root.join("source.txt"),
        "Verified overflow evidence\n".repeat(200),
    )
    .unwrap();
    submit(
        &fixture.client,
        fixture.session.id,
        "Read source.txt".into(),
    )
    .await;
    submit(&fixture.client, fixture.session.id, "Second".into()).await;
    // The two latest completed turns stay verbatim, so the first is summarized.
    submit(&fixture.client, fixture.session.id, "Third".into()).await;
    let (_, _, page) = submit(
        &fixture.client,
        fixture.session.id,
        "Continue after overflow".into(),
    )
    .await;
    assert_eq!(
        page.entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter(|part| matches!(part, Part::Compaction(_)))
            .count(),
        1
    );
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 7);
    assert!(agent_support::compaction::is_summary(&requests[5]));
    assert!(messages(&requests[5]).contains("Verified overflow evidence"));
    let retried = messages(&requests[6]);
    assert!(retried.contains("Context summary fixture: overflow evidence retained"));
    assert!(retried.contains("Continue after overflow"));
    assert!(!retried.contains("evidence-call"));
    assert!(retried.len() < messages(&requests[4]).len());
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}
