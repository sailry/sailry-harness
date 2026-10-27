use super::*;

#[tokio::test]
async fn retains_complete_exchanges() {
    for remote in [false, true] {
        let server = Server::native_compaction(false).await;
        let fixture = fixture(remote, &server).await;
        for index in 1..=4 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index}\n").repeat(120),
            )
            .unwrap();
        }
        let (_, _, page) = submit(
            &fixture.client,
            fixture.session.id,
            "Compact loop evidence".into(),
        )
        .await;
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::Thinking(text) if text.contains("Inspect the next evidence source"))));
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 6);
        assert!(agent_support::compaction::is_summary(&requests[4]));
        let summary = messages(&requests[4]);
        assert!(summary.contains("Evidence 1"));
        for index in 2..=4 {
            assert!(!summary.contains(&format!("Evidence {index}")));
        }
        assert!(!summary.contains("compact-call"));
        let current = messages(&requests[5]);
        assert!(!current.contains("Evidence 1"));
        for index in 2..=4 {
            assert!(current.contains(&format!("Evidence {index}")));
        }
        assert!(current.contains("compacted"));
        assert!(current.contains("compact-call"));
        assert_eq!(requests[0]["messages"][0]["role"], "system");
        assert_eq!(requests[5]["messages"][0]["role"], "system");
        assert!(
            requests[5]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| {
                    message["role"] == "user"
                        && message["content"]
                            .as_str()
                            .is_some_and(|text| text.contains("Context summary fixture"))
                })
        );
        assert_eq!(requests[0]["messages"][0], requests[5]["messages"][0]);
        let full = serde_json::to_string(&page.entries).unwrap();
        for index in 1..=4 {
            assert!(full.contains(&format!("Evidence {index}")));
        }
        usage_total(
            &fixture.client,
            6,
            500 + agent_support::compaction::NATIVE_USAGE as u64,
        )
        .await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_active_call() {
    for remote in [false, true] {
        let server = Server::native_compaction(false).await;
        let fixture = fixture(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Original evidence 中文 🙂\n".repeat(100),
        )
        .unwrap();
        submit(
            &fixture.client,
            fixture.session.id,
            "Read source.txt".into(),
        )
        .await;
        let (_, _, page) = submit(
            &fixture.client,
            fixture.session.id,
            "Compact prior evidence".into(),
        )
        .await;
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        assert!(agent_support::compaction::is_summary(&requests[3]));
        assert!(messages(&requests[3]).contains("Original evidence"));
        assert!(!messages(&requests[3]).contains("compact-call"));
        let latest = messages(&requests[4]);
        assert!(latest.contains("Context summary fixture"));
        assert!(latest.contains("compact-call"));
        assert!(latest.contains("compacted"));
        assert!(!latest.contains("Original evidence"));
        assert_eq!(requests[2]["messages"][0], requests[4]["messages"][0]);
        let full = serde_json::to_string(&page).unwrap();
        assert!(full.contains("Original evidence"));
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::Compaction(_))))
                .count(),
            1
        );
        usage_total(
            &fixture.client,
            5,
            400 + agent_support::compaction::NATIVE_USAGE as u64,
        )
        .await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_unapplied_history() {
    for remote in [false, true] {
        for fail in [false, true] {
            let server = Server::native_compaction(fail).await;
            let fixture = fixture(remote, &server).await;
            if fail {
                std::fs::write(
                    fixture.root.join("source.txt"),
                    "Original evidence\n".repeat(180),
                )
                .unwrap();
                submit(
                    &fixture.client,
                    fixture.session.id,
                    "Read source.txt".into(),
                )
                .await;
            }
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: 1,
                    message: "Compact prior evidence".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected");
            };
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                if fail {
                    Status::Failed
                } else {
                    Status::Completed
                }
            );
            assert!(!page.entries.iter().any(|entry| {
                entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::Compaction(_)))
            }));
            let requests = server.requests.lock().unwrap().clone();
            let latest = messages(requests.last().unwrap());
            if fail {
                assert!(agent_support::compaction::is_summary(
                    requests.last().unwrap()
                ));
                assert!(latest.contains("Original evidence"));
                assert!(page.queue.paused);
                assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
                    matches!(part, Part::Resource(value) if value["type"] == "compaction_failure"
                        && value["failure"]["source"] == "manual" && value["failure"]["continued"] == false)
                }));
            } else {
                assert!(latest.contains("skipped"));
            }
            assert_eq!(requests.len(), if fail { 4 } else { 2 });
            let responses = if fail { 3 } else { 2 };
            usage_total(
                &fixture.client,
                responses,
                (responses - 1) * 100 + agent_support::compaction::NATIVE_USAGE as u64,
            )
            .await;
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn skips_below_half_trigger() {
    for remote in [false, true] {
        let server = Server::native_compaction(false).await;
        let fixture = fixture(remote, &server).await;
        super::budget::model(&fixture, 128_000, 2_048, false).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Original evidence retained for follow-up\n".repeat(120),
        )
        .unwrap();
        submit(
            &fixture.client,
            fixture.session.id,
            "Read source.txt".into(),
        )
        .await;
        let (_, _, page) = submit(
            &fixture.client,
            fixture.session.id,
            "Compact prior evidence".into(),
        )
        .await;
        let results = tool_details(&fixture.client, &page).await;
        let result = results
            .into_iter()
            .find(|(name, _)| name == "compact_context")
            .unwrap()
            .1
            .result
            .unwrap();
        assert_eq!(result["status"], "skipped");
        assert!(
            result["context_tokens"].as_u64().unwrap() * 2
                < result["trigger_tokens"].as_u64().unwrap()
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        assert!(
            requests
                .iter()
                .all(|request| !agent_support::compaction::is_summary(request))
        );
        assert!(messages(requests.last().unwrap()).contains("Original evidence"));
        assert!(
            !page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Compaction(_)))
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn continues_after_recoverable_failure() {
    for remote in [false, true] {
        for (status, reason, attempts) in [
            (Some(503), "service_rejected", 2),
            (Some(429), "rate_limited", 2),
            (None, "no_reduction", 1),
        ] {
            let server = Server::native_summary(
                status,
                "An expanding handoff with no reduction. ".repeat(1500),
            )
            .await;
            let fixture = fixture(remote, &server).await;
            std::fs::write(
                fixture.root.join("source.txt"),
                "Original evidence remains available\n".repeat(180),
            )
            .unwrap();
            submit(
                &fixture.client,
                fixture.session.id,
                "Read source.txt".into(),
            )
            .await;
            let (_, _, page) = submit(
                &fixture.client,
                fixture.session.id,
                "Compact prior evidence".into(),
            )
            .await;
            assert!(!page.queue.paused);
            assert!(
                !page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| { matches!(part, Part::Compaction(_)) })
            );
            assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
                matches!(part, Part::Resource(value) if value["type"] == "compaction_failure"
                    && value["failure"]["source"] == "manual"
                    && value["failure"]["reason"] == reason
                    && value["failure"]["continued"] == true)
            }));
            let results = tool_details(&fixture.client, &page).await;
            let result = results
                .iter()
                .find(|(name, _)| name == "compact_context")
                .unwrap()
                .1
                .result
                .as_ref()
                .unwrap();
            assert_eq!(result["status"], "failed");
            assert_eq!(
                results.len(),
                2,
                "the read and compaction each execute once"
            );
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 4 + attempts);
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| agent_support::compaction::is_summary(request))
                    .count(),
                attempts
            );
            let retained = messages(requests.last().unwrap());
            assert!(retained.contains("Original evidence remains available"));
            assert!(retained.contains("compact-call"));
            assert!(retained.contains("failed"));
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn allows_automatic_compaction_after_growth() {
    for remote in [false, true] {
        let server = Server::native_compaction(false).await;
        let fixture = fixture(remote, &server).await;
        for index in 1..=5 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index}\n").repeat(if index == 5 { 2 } else { 120 }),
            )
            .unwrap();
        }
        let (_, _, page) = submit(
            &fixture.client,
            fixture.session.id,
            "Compact loop evidence. Continue after explicit compaction".into(),
        )
        .await;
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(
            requests
                .iter()
                .filter(|request| agent_support::compaction::is_summary(request))
                .count(),
            2
        );
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::Compaction(_)))
                .count(),
            2
        );
        let latest = messages(requests.last().unwrap());
        assert!(latest.contains("Evidence 4"));
        assert!(latest.contains("Evidence 5"));
        assert!(!latest.contains("Evidence 1"));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
