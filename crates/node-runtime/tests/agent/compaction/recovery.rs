use super::*;

#[tokio::test]
async fn retries_transient_summary_without_replaying_tools() {
    for remote in [false, true] {
        let server = Server::recovering_summary().await;
        let fixture = Fixture::files(remote, &server).await;
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
async fn does_not_retry_invalid_overfull_summary() {
    for remote in [false, true] {
        let server =
            Server::compaction_sequence(false, String::new(), vec![100, 20_000, 100]).await;
        let fixture = Fixture::files(remote, &server).await;
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
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: "Continue with verified evidence".into(),
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
        assert!(
            !after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Compaction(_)))
        );
        assert!(after.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::Resource(value) if value["type"] == "compaction_failure" && value["failure"]["continued"] == false)));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
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
            let fixture = Fixture::files(remote, &server).await;
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
            let db =
                rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                    .unwrap();
            if storage_failure {
                db.execute_batch("CREATE TRIGGER reject_summary BEFORE INSERT ON agent_events WHEN json_type(NEW.body,'$.actions.compaction')='object' BEGIN SELECT RAISE(ABORT,'PRIVATE_SAVE_FAILURE_SENTINEL'); END;").unwrap();
            }
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::CompactContext {
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
            let count: i64 = db.query_row("SELECT count(*) FROM agent_events WHERE json_type(body,'$.actions.compaction')='object'", [], |row| row.get(0)).unwrap();
            assert_eq!(count, 0);
            if storage_failure {
                db.execute_batch("DROP TRIGGER reject_summary").unwrap();
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
