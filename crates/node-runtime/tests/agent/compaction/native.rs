use super::*;

#[tokio::test]
async fn retains_complete_exchanges() {
    for remote in [false, true] {
        let server = Server::native_compaction(false).await;
        let fixture = Fixture::new(remote, &server).await;
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
        assert_eq!(requests[0]["messages"][0], requests[5]["messages"][0]);
        let full = serde_json::to_string(&page.entries).unwrap();
        for index in 1..=4 {
            assert!(full.contains(&format!("Evidence {index}")));
        }
        usage(&fixture.client, 6).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_active_call() {
    for remote in [false, true] {
        let server = Server::native_compaction(false).await;
        let fixture = Fixture::new(remote, &server).await;
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
        usage(&fixture.client, 5).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_unapplied_history() {
    for remote in [false, true] {
        for fail in [false, true] {
            let server = Server::native_compaction(fail).await;
            let fixture = Fixture::new(remote, &server).await;
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
            let (_, _, page) = submit(
                &fixture.client,
                fixture.session.id,
                "Compact prior evidence".into(),
            )
            .await;
            assert!(!page.entries.iter().any(|entry| {
                entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::Compaction(_)))
            }));
            let requests = server.requests.lock().unwrap().clone();
            let latest = messages(requests.last().unwrap());
            assert!(latest.contains(if fail {
                "context summary request failed"
            } else {
                "skipped"
            }));
            if fail {
                assert!(latest.contains("Original evidence"));
            }
            assert_eq!(requests.len(), if fail { 5 } else { 2 });
            usage(&fixture.client, if fail { 4 } else { 2 }).await;
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
