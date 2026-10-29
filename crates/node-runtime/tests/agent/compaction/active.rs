use super::*;

#[tokio::test]
async fn resumes_without_replay() {
    for remote in [false, true] {
        let server =
            Server::loop_usage(vec![Some(100), Some(100), Some(26_000), Some(100)], 100).await;
        let fixture = fixture(remote, &server).await;
        for index in 1..=6 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index} 中文 🙂\n").repeat(200),
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
        assert_eq!(requests.len(), 8);
        assert_eq!(
            requests
                .iter()
                .filter(|request| agent_support::compaction::is_summary(request))
                .count(),
            1
        );
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::Compaction(_)))
                .count(),
            1
        );
        let full = serde_json::to_string(&tool_details(&fixture.client, &page).await).unwrap();
        for index in 1..=6 {
            assert!(full.contains(&format!("Evidence {index}")));
        }
        fixture.client.execute(request.clone()).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), requests.len());
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
        let node = Node::start_empty(fixture.directory.path().join("node"))
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
        assert_eq!(server.requests.lock().unwrap().len(), requests.len());
        for session in [fixture.session.id, branch.id] {
            let count = server.requests.lock().unwrap().len();
            submit(&client, session, "Continue after restart".into()).await;
            assert_eq!(server.requests.lock().unwrap().len(), count + 1);
            let latest = server.requests.lock().unwrap().last().unwrap().clone();
            assert!(messages(&latest).contains("Context summary fixture"));
            assert!(messages(&latest).contains("Continue after restart"));
        }
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn cancels_before_next_model_call() {
    for remote in [false, true] {
        let server = Server::pending_loop_compaction().await;
        let fixture = fixture(remote, &server).await;
        for index in 1..=6 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index}\n").repeat(200),
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
        server.wait_count(2).await;
        assert!(agent_support::compaction::is_summary(
            server.requests.lock().unwrap().last().unwrap()
        ));
        let before = history(&fixture.client, fixture.session.id).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        let after = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            after
                .runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Cancelled
        );
        assert_eq!(after.entries, before.entries);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert!(
            !after
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Compaction(_)))
        );
        let full = serde_json::to_string(&tool_details(&fixture.client, &after).await).unwrap();
        assert!(full.contains("Evidence 1"));
        assert!(!full.contains("Evidence 2"));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_context_on_failure() {
    for remote in [false, true] {
        let server = Server::loop_compaction(true).await;
        let fixture = fixture(remote, &server).await;
        for index in 1..=6 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index}\n").repeat(200),
            )
            .unwrap();
        }
        let request = fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: 1,
            message: "Verify six files".into(),
        });
        let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
        assert_eq!(run.status, Status::Failed);
        let error = run.error.as_ref().unwrap();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert_eq!(
            error.details,
            Some(FaultDetails::ModelRequest {
                reason: ModelFailure::Request
            })
        );
        assert!(
            error
                .message
                .starts_with("Ran into this error trying to compact:")
        );
        assert!(!page.queue.paused);
        assert!(
            !page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Compaction(_)))
        );
        let requests = server.requests.lock().unwrap().clone();
        let attempts = goose_provider_types::retry::RetryConfig::default().max_retries + 1;
        assert_eq!(requests.len(), 1 + attempts);
        assert!(!agent_support::compaction::is_summary(&requests[0]));
        for request in &requests[1..] {
            assert!(agent_support::compaction::is_summary(request));
            assert!(request["tools"].as_array().is_none_or(Vec::is_empty));
            assert!(messages(request).contains("Verify six files"));
            assert!(messages(request).contains("Evidence 1"));
        }
        let full = serde_json::to_string(&tool_details(&fixture.client, &page).await).unwrap();
        assert!(full.contains("Evidence 1"));
        assert!(!full.contains("Evidence 2"));
        fixture.client.execute(request).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 1 + attempts);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_read_and_write_results() {
    for remote in [false, true] {
        let server = Server::file_compaction().await;
        let fixture = fixture(remote, &server).await;
        for index in 1..=3 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index}\n").repeat(600),
            )
            .unwrap();
        }
        let request = fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: 1,
            message: "Read the sources and write written.txt once".into(),
        });
        let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("turn expected")
        };
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        // The real parent permission check precedes dispatch. Ordinary file
        // tools retain their existing sequential execution policy.
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: fixture.session.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("written.txt")).unwrap(),
            "Written exactly once"
        );
        let requests = server.requests.lock().unwrap().clone();
        assert!(requests.iter().any(agent_support::compaction::is_summary));
        assert!(messages(requests.last().unwrap()).contains("Context summary fixture"));
        let full = serde_json::to_string(&tool_details(&fixture.client, &page).await).unwrap();
        for text in [
            "Evidence 1",
            "Evidence 2",
            "Evidence 3",
            "Written exactly once",
        ] {
            assert!(full.contains(text), "{text}");
        }
        let writes = page.entries.iter().flat_map(|entry| &entry.parts).filter(|part| matches!(part, Part::ToolCall {name,..} if name == &plugin_tool("files","write_file"))).count();
        assert_eq!(writes, 1);
        fixture.client.execute(request).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), requests.len());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn resumes_out_of_order_delegation_results() {
    for remote in [false, true] {
        let parent = Server::delegate_compaction().await;
        let release = sailry_link::CancellationToken::new();
        let held = Server::markdown_held("First child evidence".into(), release.clone()).await;
        let fast = Server::markdown("Second child evidence".into()).await;
        let mut fixture =
            super::super::delegation::fixture::Fixture::start(remote, parent, held).await;
        for index in 1..=2 {
            std::fs::write(
                fixture.root().join(format!("source-{index}.txt")),
                format!("Evidence {index}\n").repeat(600),
            )
            .unwrap();
        }
        // The parent keeps the compact trigger used before the 128k delegation model.
        let mut parent = configuration::snapshot(&fixture.client)
            .await
            .providers
            .into_iter()
            .find(|provider| provider.id == fixture.session.config.provider)
            .expect("the session's parent provider");
        parent
            .models
            .iter_mut()
            .find(|model| model.id == fixture.session.config.model)
            .expect("the parent session model")
            .compaction_tokens = Some(24_000);
        let Output::Provider(_) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: parent.revision,
                provider: parent,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let mut provider = fixture.provider.clone();
        provider.id = sailry_protocol::ProviderId::new();
        provider.revision = 0;
        provider.endpoint = fast.endpoint.clone();
        let Output::Provider(provider) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider,
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let mut role = fixture.role.clone();
        role.id = sailry_protocol::RoleId::new();
        role.revision = 0;
        role.key = "fast".into();
        role.name = "Fast".into();
        role.model.as_mut().unwrap().provider = provider.id;
        let Output::Role(role) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutRole {
                role,
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("role expected")
        };
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionRoles {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                roles: vec![fixture.role.reference(), role.reference()],
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let request = fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Read both files and delegate the independent tasks".into(),
        });
        let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("turn expected")
        };
        fixture.child.wait_count(1).await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                if page
                    .children
                    .iter()
                    .any(|child| child.run.status == Status::Completed)
                {
                    assert!(
                        page.children
                            .iter()
                            .any(|child| child.run.status == Status::Running)
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the second child must finish while the first remains held");
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert_eq!(page.children.len(), 2);
        assert!(
            page.children
                .iter()
                .all(|child| child.run.status == Status::Completed)
        );
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert!(requests.iter().any(agent_support::compaction::is_summary));
        let resumed = messages(requests.last().unwrap());
        for text in [
            "Context summary fixture",
            "First child evidence",
            "Second child evidence",
        ] {
            assert!(resumed.contains(text), "{text}");
        }
        fixture.client.execute(request).await.unwrap();
        assert_eq!(
            fixture.parent.requests.lock().unwrap().len(),
            requests.len()
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        assert_eq!(fast.requests.lock().unwrap().len(), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
