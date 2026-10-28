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
async fn rejects_failed_saves_without_advancing() {
    for remote in [false, true] {
        let server = Server::compaction(false).await;
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
        fixture.node.with_storage(|db|Box::pin(async move { db.execute_batch("CREATE TRIGGER reject_summary BEFORE INSERT ON agent_events WHEN json_extract(NEW.body,'$.kind')='compaction' BEGIN SELECT RAISE(ABORT,'PRIVATE_SAVE_FAILURE_SENTINEL'); END;").await.map_err(crate::database_fixture::fault) })).await.unwrap();
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
            panic!("turn expected")
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
        assert!(
            !serde_json::to_string(&after)
                .unwrap()
                .contains("PRIVATE_SAVE_FAILURE_SENTINEL")
        );
        let count:i64=fixture.node.with_storage(|db|Box::pin(async move { db.prepare("SELECT count(*) FROM agent_events WHERE json_extract(body,'$.kind')='compaction'").await.map_err(crate::database_fixture::fault)?.query_row(()).await.map_err(crate::database_fixture::fault)?.get(0).map_err(crate::database_fixture::fault) })).await.unwrap();
        assert_eq!(count, 0);
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
        assert!(messages(requests.last().unwrap()).contains("Verified source evidence"));
        assert!(messages(requests.last().unwrap()).contains("evidence-call"));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn compacts_after_provider_overflow() {
    for remote in [false, true] {
        for repeated in [false, true] {
            let server = if repeated {
                Server::repeated_overflow().await
            } else {
                Server::context_overflow().await
            };
            let fixture = fixture(remote, &server).await;
            for (path, text) in [
                ("source.txt", "Verified overflow evidence"),
                ("middle.txt", "Retained middle file evidence"),
                ("recent.txt", "Retained recent file evidence"),
            ] {
                std::fs::write(fixture.root.join(path), format!("{text}\n").repeat(120)).unwrap();
            }
            for task in [
                "Read source.txt",
                "Read a middle result before overflow",
                "Read a recent result before overflow",
            ] {
                submit(&fixture.client, fixture.session.id, task.into()).await;
            }
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: 1,
                    message: "Continue after overflow".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                if repeated {
                    Status::Failed
                } else {
                    Status::Completed
                }
            );
            let requests = server.requests.lock().unwrap().clone();
            let summaries: Vec<_> = requests
                .iter()
                .filter(|request| agent_support::compaction::is_summary(request))
                .collect();
            assert_eq!(summaries.len(), if repeated { 2 } else { 1 });
            assert_eq!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .filter(|part| matches!(part, Part::Compaction(_)))
                    .count(),
                summaries.len()
            );
            let source = messages(summaries[0]);
            for text in [
                "Verified overflow evidence",
                "Retained middle file evidence",
                "Retained recent file evidence",
            ] {
                assert!(source.contains(text), "{text}");
            }
            if repeated {
                assert_eq!(requests.len(), 11);
                assert!(!page.queue.paused);
            } else {
                assert_eq!(requests.len(), 9);
                let retried = messages(requests.last().unwrap());
                assert!(retried.contains("Context summary fixture: overflow evidence retained"));
                assert!(retried.contains("Continue after overflow"));
                assert!(!retried.contains("evidence-call"));
            }
            let full = serde_json::to_string(&tool_details(&fixture.client, &page).await).unwrap();
            for text in [
                "Verified overflow evidence",
                "Retained middle file evidence",
                "Retained recent file evidence",
            ] {
                assert!(full.contains(text), "{text}");
            }
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
