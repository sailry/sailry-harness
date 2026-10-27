use super::*;

#[tokio::test]
async fn reuses_masked_prefix_for_later_summary() {
    for remote in [false, true] {
        let server = Server::loop_usage(
            vec![
                Some(100),
                Some(100),
                Some(100),
                Some(100),
                Some(100),
                Some(26_000),
                Some(100),
            ],
            100,
        )
        .await;
        let fixture = fixture(remote, &server).await;
        for index in 1..=6 {
            let bytes = if index == 3 { 100 } else { 16 } * 1024;
            let mut text = format!("Evidence {index}\n");
            text.push_str(&"x".repeat(bytes - text.len()));
            std::fs::write(fixture.root.join(format!("source-{index}.txt")), text).unwrap();
        }
        submit(
            &fixture.client,
            fixture.session.id,
            "Preserve all evidence across context windows".into(),
        )
        .await;
        let requests = server.requests.lock().unwrap().clone();
        let summaries: Vec<_> = requests
            .iter()
            .filter(|request| agent_support::compaction::is_summary(request))
            .collect();
        assert_eq!(summaries.len(), 2);
        let normal = requests
            .iter()
            .filter(|request| !agent_support::compaction::is_summary(request))
            .find(|request| {
                request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| {
                        message["tool_call_id"] == "loop-3"
                            && message["content"]
                                .as_str()
                                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                                .is_some_and(|value| value["masked"] == true)
                    })
            })
            .expect("an ordinary request with the retained masked result");
        let prefix = normal["messages"].as_array().unwrap();
        let summary = summaries[1]["messages"].as_array().unwrap();
        assert_eq!(
            serde_json::to_vec(prefix).unwrap(),
            serde_json::to_vec(&summary[..prefix.len()]).unwrap()
        );
        assert_eq!(normal["tools"], summaries[1]["tools"]);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn resumes_without_replay() {
    for remote in [false, true] {
        let server = Server::loop_compaction(false).await;
        let fixture = fixture(remote, &server).await;
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
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::Thinking(text) if text.contains("Inspect the next evidence source"))));
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
        let prefix = ordinary[0]["messages"].as_array().unwrap();
        let summary = summaries[0]["messages"].as_array().unwrap();
        assert_eq!(
            serde_json::to_vec(prefix).unwrap(),
            serde_json::to_vec(&summary[..prefix.len()]).unwrap(),
        );
        assert_eq!(ordinary[0]["tools"], summaries[0]["tools"]);
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
        usage_total(&fixture.client, 11, 7 * 26000 + 4 * 100).await;
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
        let fixture = fixture(remote, &server).await;
        for index in 1..=6 {
            std::fs::write(
                fixture.root.join(format!("source-{index}.txt")),
                format!("Evidence {index}\n").repeat(1000),
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
        usage_total(&fixture.client, 3, 3 * 26000).await;
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
        let mut stream = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let observer = tokio::spawn(async move {
            let mut warnings = Vec::new();
            while let Ok(update) = stream.next().await {
                if let Update::ConversationFrame(frame) = update {
                    match frame.change {
                        sailry_protocol::conversation::Change::Entry(entry) => {
                            for part in &entry.parts {
                                if let Part::Resource(value) = part
                                    && value["type"] == "compaction_failure"
                                {
                                    assert_eq!(value["failure"]["source"], "automatic");
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
                                    assert_eq!(value["failure"]["status"], 400);
                                    assert_eq!(value["failure"]["continued"], false);

                                    warnings.push(entry.id.clone());
                                }
                            }
                        }
                        sailry_protocol::conversation::Change::Run(run)
                            if run.status == Status::Failed =>
                        {
                            break;
                        }
                        _ => {}
                    }
                }
            }
            warnings
        });
        let request = fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: 1,
            message: "Verify six files".into(),
        });
        let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
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
            Status::Failed
        );
        assert!(page.queue.paused);
        assert!(!page.entries.iter().any(|entry| {
            entry
                .parts
                .iter()
                .any(|part| matches!(part, Part::Compaction(_)))
        }));
        let live = tokio::time::timeout(Duration::from_secs(10), observer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(live.len(), 1);
        let notices = super::super::configuration::snapshot(&fixture.client)
            .await
            .notifications;
        let diagnostics: Vec<_> = notices
            .iter()
            .filter_map(|notice| match &notice.content.details {
                Some(sailry_protocol::notification::Details::Diagnostic(details)) => Some(details),
                _ => None,
            })
            .collect();
        assert_eq!(diagnostics.len(), live.len());
        assert!(
            diagnostics
                .iter()
                .all(|details| live.contains(&details.event))
        );

        assert_eq!(
            live.iter().collect::<std::collections::BTreeSet<_>>().len(),
            1
        );
        let mut restored = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let snapshot = restored.next().await.unwrap();
        let mut projection = sailry_client::conversation::Projection::new(
            fixture.client.target(),
            fixture.session.id,
            1,
        );
        projection.apply(1, snapshot).unwrap();
        assert_eq!(projection.live_start(), None);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(
            requests
                .iter()
                .filter(|request| !agent_support::compaction::is_summary(request))
                .count(),
            3
        );
        // The terminal summary failure halts before any further model or tool work.
        let summaries: Vec<_> = requests
            .iter()
            .filter(|request| agent_support::compaction::is_summary(request))
            .collect();
        assert_eq!(summaries.len(), 1);
        assert_eq!(page.entries.iter().flat_map(|entry| &entry.parts).filter(|part| matches!(part, Part::Resource(value) if value["type"] == "compaction_failure")).count(), 1);
        let content = messages(requests.last().unwrap());
        assert!(!content.contains("compaction_failure"));
        let details = tool_details(&fixture.client, &page).await;
        let full = serde_json::to_string(&details).unwrap();
        for index in 1..=3 {
            assert!(full.contains(&format!("Evidence {index}")));
        }
        assert!(!full.contains("Evidence 4"));
        fixture.client.execute(request).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        usage_total(&fixture.client, 3, 3 * 26000).await;
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
        assert_eq!(requests.len(), 5);
        assert!(agent_support::compaction::is_summary(&requests[3]));
        assert!(messages(&requests[3]).contains("Evidence 1"));
        assert!(!messages(&requests[3]).contains("Evidence 2"));
        let resumed = messages(&requests[4]);
        for text in [
            "Context summary fixture",
            "Evidence 2",
            "Evidence 3",
            "parallel-read",
            "parallel-write",
            "Written exactly once",
        ] {
            assert!(resumed.contains(text), "{text}");
        }
        assert!(!resumed.contains("Evidence 1"));
        let writes = page.entries.iter().flat_map(|entry| &entry.parts).filter(|part| matches!(part, Part::ToolCall {name,..} if name == &plugin_tool("files","write_file"))).count();
        assert_eq!(writes, 1);
        fixture.client.execute(request).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 5);
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
                let ids = page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .filter_map(|part| {
                        if let Part::ToolResult { id: Some(id), .. } = part {
                            Some(id.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                assert!(!ids.contains(&"delegated-first"));
                if ids.contains(&"delegated-second") {
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
        let results = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| {
                if let Part::ToolResult { id: Some(id), .. } = part {
                    Some(id.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert!(
            results
                .iter()
                .position(|id| *id == "delegated-second")
                .unwrap()
                < results
                    .iter()
                    .position(|id| *id == "delegated-first")
                    .unwrap()
        );
        let requests = fixture.parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        assert!(agent_support::compaction::is_summary(&requests[3]));
        assert!(messages(&requests[3]).contains("Evidence 1"));
        let resumed = messages(&requests[4]);
        for text in [
            "Context summary fixture",
            "Evidence 2",
            "delegated-first",
            "delegated-second",
            "First child evidence",
            "Second child evidence",
        ] {
            assert!(resumed.contains(text), "{text}");
        }
        let calls = requests[4]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "tool")
            .filter_map(|message| message["tool_call_id"].as_str())
            .collect::<Vec<_>>();
        assert!(
            calls
                .iter()
                .position(|id| *id == "delegated-first")
                .unwrap()
                < calls
                    .iter()
                    .position(|id| *id == "delegated-second")
                    .unwrap()
        );
        fixture.client.execute(request).await.unwrap();
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 5);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        assert_eq!(fast.requests.lock().unwrap().len(), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
