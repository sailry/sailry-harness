use super::*;

#[tokio::test]
async fn resumes_without_replay() {
    for remote in [false, true] {
        let server = Server::loop_compaction(false).await;
        let fixture = Fixture::new(remote, &server).await;
        for index in 1..=6 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index} 中文 🙂\n").repeat(1000),
            )
            .unwrap();
        }
        let (request, turn, page) = submit(
            &fixture.client,
            fixture.session.id,
            "Verify all six files and preserve evidence".into(),
        )
        .await;
        let requests = server.requests.lock().unwrap().clone();
        let summaries: Vec<_> = requests
            .iter()
            .filter(|request| agent_support::compaction::is_summary(request))
            .collect();
        assert_eq!(summaries.len(), 4);
        let ordinary: Vec<_> = requests
            .iter()
            .filter(|request| !agent_support::compaction::is_summary(request))
            .collect();
        assert_eq!(ordinary.len(), 7);
        for (index, request) in ordinary.iter().enumerate().skip(3) {
            let content = messages(request);
            assert!(content.contains("Context summary fixture"));
            assert!(content.contains(&format!("Evidence {index}")));
            assert!(content.contains(&format!("Evidence {}", index - 1)));
            assert!(!content.contains(&format!("Evidence {}", index - 2)));
            assert_eq!(request["messages"][0], ordinary[0]["messages"][0]);
            assert!(content.len() < messages(ordinary[2]).len() + 3000);
        }
        for (index, request) in summaries.iter().enumerate() {
            let content = messages(request);
            assert!(content.contains(&format!("Evidence {}", index + 1)));
            assert!(!content.contains(&format!("Evidence {}", index + 2)));
            if index > 0 {
                assert!(content.contains("Context summary fixture"));
            }
        }
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::Compaction(_))))
                .count(),
            4
        );
        let full = serde_json::to_string(&page).unwrap();
        for index in 1..=6 {
            assert!(full.contains(&format!("Evidence {index}")));
        }
        usage_total(&fixture.client, 11, 7 * 14000 + 4 * 100).await;
        fixture.client.execute(request.clone()).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 11);
        let Output::Session(branch) = fixture
            .client
            .execute(fixture.client.prepare(Command::ForkConversation {
                session: fixture.session.id,
                through: turn,
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
            page.entries
        );
        client.execute(request).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 11);
        for session in [fixture.session.id, branch.id] {
            submit(&client, session, "Continue after restart".into()).await;
            let latest = server.requests.lock().unwrap().last().unwrap().clone();
            let content = messages(&latest);
            assert!(content.contains("Context summary fixture"));
            assert!(content.contains("Evidence 5"));
            assert!(content.contains("Evidence 6"));
            assert!(!content.contains("Evidence 1"));
        }
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn cancels_before_next_model_call() {
    for remote in [false, true] {
        let server = Server::pending_loop_compaction().await;
        let fixture = Fixture::new(remote, &server).await;
        for index in 1..=6 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index}\n").repeat(1800),
            )
            .unwrap();
        }
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: "Verify six files".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        server.wait_count(4).await;
        assert!(agent_support::compaction::is_summary(
            server.requests.lock().unwrap().last().unwrap()
        ));
        let before = history(&fixture.client, fixture.session.id).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Cancelled);
        assert_eq!(page.entries, before.entries);
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        let content = serde_json::to_string(&page.entries).unwrap();
        for index in 1..=3 {
            assert!(content.contains(&format!("Evidence {index}")));
        }
        assert!(!content.contains("Evidence 4"));
        usage_total(&fixture.client, 3, 3 * 14000).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_context_on_failure() {
    for remote in [false, true] {
        let server = Server::loop_compaction(true).await;
        let fixture = Fixture::new(remote, &server).await;
        for index in 1..=6 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index}\n").repeat(1800),
            )
            .unwrap();
        }
        let (_, _, page) = submit(
            &fixture.client,
            fixture.session.id,
            "Verify six files".into(),
        )
        .await;
        assert!(!page.entries.iter().any(|entry| {
            entry
                .parts
                .iter()
                .any(|part| matches!(part, Part::Compaction(_)))
        }));
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(
            requests
                .iter()
                .filter(|request| !agent_support::compaction::is_summary(request))
                .count(),
            7
        );
        let content = messages(requests.last().unwrap());
        for index in 1..=6 {
            assert!(content.contains(&format!("Evidence {index}")));
        }
        usage_total(&fixture.client, 7, 7 * 14000).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
