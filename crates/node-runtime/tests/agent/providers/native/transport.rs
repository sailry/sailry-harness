use super::*;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;
use server::Mode;
use std::sync::atomic::Ordering;
#[path = "transport/observations.rs"]
mod observations;
#[path = "transport/preparation.rs"]
mod preparation;
#[path = "transport/recovery.rs"]
mod recovery;

#[tokio::test]
async fn preserves_protocol_error_causes() {
    for remote in [false, true] {
        for (status, advised) in [(400, false), (429, false), (429, true)] {
            let server = Server::transport(
                ModelApi::Responses,
                Reply::Text,
                Mode::Protocol { status, advised },
            )
            .await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            let turn = fixture.submit("Observe the protocol error").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            let run = page.runs.last().unwrap();
            if status == 429 {
                assert_eq!(run.status, Status::Completed);
                assert_eq!(completion::response(&page), server::ANSWER);
                assert_eq!(super::retry::attempts(&page), [1]);
            } else {
                assert_eq!(run.status, Status::Failed);
                let fault = run.error.as_ref().unwrap();
                assert_eq!(fault.message, "Protocol failure; authorization [redacted]");
                assert!(matches!(
                    fault.details,
                    Some(sailry_protocol::FaultDetails::ModelRecovery {
                        reason: sailry_protocol::conversation::recovery::Stop::InvalidPrompt
                    })
                ));
                assert!(super::retry::attempts(&page).is_empty());
            }
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), if status == 429 { 2 } else { 1 });
            assert!(
                requests
                    .iter()
                    .all(|request| request.body["type"] == "response.create")
            );
            assert_eq!(server.upgrades.lock().unwrap().len(), 1);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn reuses_after_provider_error() {
    for remote in [false, true] {
        let server = Server::transport(ModelApi::Responses, Reply::Text, Mode::ErrorOnce).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Retry the provider rejection").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert_eq!(completion::response(&page), server::ANSWER);
        assert_eq!(super::retry::attempts(&page), [1]);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| request.body["type"] == "response.create")
        );
        // A terminal response.failed closes this response, so the socket can be reused.
        assert_eq!(server.upgrades.lock().unwrap().len(), 1);
        assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn switches_after_pre_event_protocol_errors() {
    for remote in [false, true] {
        let server = Server::transport(ModelApi::Responses, Reply::Text, Mode::InvalidFrame).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Recover the interrupted generation").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert_eq!(completion::response(&page), server::ANSWER);
        assert_eq!(super::retry::attempts(&page), [1]);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.body["type"] == "response.create")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.body.get("type").is_none())
                .count(),
            1
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn keeps_compaction_on_http() {
    for remote in [false, true] {
        let mut event = server::response_events(false, false).pop().unwrap();
        event["response"]["output"][0]["content"][0]["text"] =
            serde_json::json!(crate::agent_support::compaction::handoff(server::ANSWER));
        let reply = Reply::Raw(Arc::new(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {event}\n\ndata: [DONE]\n\n"
        ).into_bytes()));
        let server = Server::transport(ModelApi::Responses, reply, Mode::Accept).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let mut provider = fixture.provider.clone();
        provider.models[0].context = 32_000;
        provider.models[0].output = 4_096;
        execute(
            &fixture.client,
            Command::SaveProvider {
                expected_revision: provider.revision,
                provider,
                secret: None,
            },
        )
        .await;
        for index in 0..3 {
            let turn = fixture
                .submit(&format!("Context evidence {index} ").repeat(100))
                .await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
        }
        let Output::QueuedTurn(summary) = execute(
            &fixture.client,
            Command::CompactContext {
                focus: None,
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
            },
        )
        .await
        else {
            panic!("summary turn expected");
        };
        let page = finished(&fixture.client, fixture.session.id, summary.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Compaction(_)))
        );
        let turn = fixture.submit("Continue after the summary").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        assert!(
            requests[..3]
                .iter()
                .all(|request| request.body["type"] == "response.create")
        );
        let summary = &requests[3];
        assert!(summary.body.get("type").is_none());
        let handoff = summary.body["input"].as_array().unwrap().last().unwrap();
        assert_eq!(handoff["role"], "user");
        assert!(
            handoff["content"].as_str().unwrap().starts_with(
                "Update the existing handoff, if any (the earlier user message containing the line `--- End of context handoff ---`), using the canonical conversation prefix above."
            )
        );
        assert_eq!(summary.body["store"], false);
        assert_eq!(
            summary.headers["authorization"],
            format!("Bearer {}", server::KEY)
        );
        assert_eq!(requests[4].body["type"], "response.create");
        assert_eq!(server.upgrades.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

async fn stream(remote: bool, mode: Mode) -> Vec<(Status, Vec<Part>)> {
    let release = Arc::new(tokio::sync::Notify::new());
    let server = Server::transport(ModelApi::Responses, Reply::Stream(release.clone()), mode).await;
    let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
    let (updates, mut observed) = tokio::sync::watch::channel(View::default());
    let stop = CancellationToken::new();
    let (_requests, requests) = tokio::sync::mpsc::channel(1);
    let watcher =
        fixture
            .client
            .watch_conversation(fixture.session.id, updates, stop.clone(), requests);
    let verify = async {
        let turn = fixture.submit("Stream the answer").await;
        let partial = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(snapshot) = &observed.borrow().snapshot
                    && let Some(draft) = snapshot
                        .drafts
                        .iter()
                        .find(|draft| draft.turn == turn && !draft.parts.is_empty())
                {
                    break draft.parts.clone();
                }
                observed.changed().await.unwrap();
            }
        })
        .await
        .expect("streamed draft deadline");
        assert_eq!(partial, [Part::Text(server::ANSWER.into())]);
        release.notify_one();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        let parts: Vec<_> = page
            .entries
            .iter()
            .filter(|entry| entry.author != "user")
            .flat_map(|entry| entry.parts.iter())
            .filter(|part| matches!(part, Part::Text(_)))
            .cloned()
            .collect();
        let statistics = crate::observations::statistics(&fixture.client, fixture.session.id).await;
        let observation = statistics.observations.recent.last().unwrap();
        assert_eq!(observation.turn, turn);
        let attempt = observation.attempts.last().unwrap();
        let first = attempt.first_text_us.expect("streamed text delivery");
        let response = attempt.response_us.expect("API response receipt");
        assert!(response <= first);
        assert!(first < attempt.elapsed_us.unwrap());
        assert_eq!(statistics.observations.latency.first_text_us, Some(first));
        assert_eq!(
            statistics.observations.latency.waiting_us,
            Some(first - response)
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let upgrades = server.upgrades.lock().unwrap();
        assert_eq!(upgrades.len(), 1);
        assert!(upgrades[0].received <= requests[0].received);
        assert_eq!(
            upgrades[0].headers["authorization"],
            format!("Bearer {}", server::KEY)
        );
        assert_eq!(
            upgrades[0].headers["session-id"],
            fixture.session.id.to_string()
        );
        assert_eq!(
            upgrades[0].headers["openai-beta"],
            "responses_websockets=2026-02-06"
        );
        assert_eq!(requests[0].body["store"], false);
        assert!(
            requests[0]
                .body
                .to_string()
                .contains("reasoning.encrypted_content")
        );
        stop.cancel();
        vec![(Status::Running, partial), (Status::Completed, parts)]
    };
    let (result, trace) = tokio::join!(watcher, verify);
    result.unwrap();
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
    trace
}

#[tokio::test]
async fn streams_canonical_events() {
    let expected = vec![
        (Status::Running, vec![Part::Text(server::ANSWER.into())]),
        (Status::Completed, vec![Part::Text(server::ANSWER.into())]),
    ];
    for remote in [false, true] {
        for mode in [Mode::Refuse(426), Mode::Accept] {
            assert_eq!(stream(remote, mode).await, expected);
        }
    }
}

#[tokio::test]
async fn continues_the_canonical_tool_loop() {
    for remote in [false, true] {
        for mode in [Mode::Refuse(426), Mode::Accept] {
            let server = Server::transport(ModelApi::Responses, Reply::Tool, mode).await;
            let fixture =
                Fixture::packages(remote, ModelApi::Responses, &server.endpoint, &["files"]).await;
            std::fs::write(fixture.root.join("native.txt"), "工具内容 🙂").unwrap();
            let turn = fixture.submit("Read native.txt").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert!(text(&page).contains(server::ANSWER));
            assert!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part, Part::ToolResult { .. }))
            );
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 2);
            assert_eq!(server.upgrades.lock().unwrap().len(), 1);
            assert!(requests[1].body.to_string().contains("工具内容 🙂"));
            match mode {
                Mode::Accept => {
                    assert_eq!(
                        requests[1].body["previous_response_id"],
                        "native-response",
                        "{:?}",
                        requests
                            .iter()
                            .map(|request| &request.body)
                            .collect::<Vec<_>>()
                    );
                    let input = requests[1].body["input"].as_array().unwrap();
                    assert!(input.iter().all(|item| item["type"] != "reasoning"));
                    assert!(
                        input
                            .iter()
                            .any(|item| item["type"] == "function_call_output")
                    );
                }
                _ => {
                    assert!(
                        requests[1]
                            .body
                            .to_string()
                            .contains("fixture-encrypted-reasoning")
                    );
                    assert!(requests[1].body.get("previous_response_id").is_none());
                }
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn switches_before_repeating_requests() {
    for remote in [false, true] {
        for mode in [
            Mode::Refuse(404),
            Mode::Refuse(426),
            Mode::Refuse(401),
            Mode::Close(1003),
            Mode::Close(1008),
            Mode::Close(1013),
            Mode::Limit,
        ] {
            let server = Server::transport(ModelApi::Responses, Reply::Text, mode).await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            for _ in 0..2 {
                let turn = fixture.submit("Answer the request").await;
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                assert_eq!(
                    page.runs.last().unwrap().status,
                    Status::Completed,
                    "{:?}",
                    page.runs
                );
                assert!(text(&page).contains(server::ANSWER));
                assert_eq!(super::retry::attempts(&page), [1]);
            }
            assert_eq!(server.upgrades.lock().unwrap().len(), 1);
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.body["type"] != "response.create")
                    .count(),
                2
            );
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn stops_http_authentication_failures() {
    let reply = Reply::Dynamic(Arc::new(|index| {
        let bytes = if index == 0 {
            format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{{\"error\":{{\"code\":\"invalid_api_key\",\"message\":\"{}\"}}}}",
                server::KEY
            )
        } else {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {}\n\n",
                server::response_events(false, false).last().unwrap()
            )
        };
        server::Script {
            bytes: bytes.into_bytes(),
            ending: server::Ending::Close,
        }
    }));
    let server = Server::transport(ModelApi::Responses, reply, Mode::Refuse(401)).await;
    let fixture = Fixture::empty(false, ModelApi::Responses, &server.endpoint).await;
    let turn = fixture.submit("Use the current authorization").await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Failed);
    assert_eq!(super::retry::attempts(&page), [1]);
    assert_eq!(
        page.runs.last().unwrap().error.as_ref().unwrap().details,
        Some(FaultDetails::ModelRequest {
            reason: sailry_protocol::ModelFailure::Unauthorized,
        })
    );
    let reference = fixture.session.config.credential.as_ref().unwrap();
    let Output::Credentials(credentials) = execute(&fixture.client, Command::ListCredentials).await
    else {
        panic!("credentials expected")
    };
    let revision = credentials
        .iter()
        .find(|credential| credential.id == reference.id)
        .unwrap()
        .revision;
    let renewed = "renewed-native-provider-key";
    let Output::Credential(credential) = execute(
        &fixture.client,
        Command::PutCredential {
            id: reference.id,
            provider: fixture.provider.id,
            expected_revision: revision,
            secret: Secret::new(renewed.into()),
            expires_at_ms: None,
        },
    )
    .await
    else {
        panic!("credential expected")
    };
    assert_eq!(credential.revision, revision + 1);
    let Output::Providers(providers) = execute(&fixture.client, Command::ListProviders).await
    else {
        panic!("providers expected")
    };
    assert_eq!(
        providers
            .iter()
            .find(|provider| provider.id == fixture.provider.id)
            .unwrap()
            .revision,
        fixture.provider.revision
    );
    let turn = fixture.submit("Use the renewed authorization").await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{:?}",
        page.runs
    );
    let upgrades = server.upgrades.lock().unwrap().clone();
    assert_eq!(upgrades.len(), 1);
    assert_eq!(
        upgrades[0].headers["authorization"],
        format!("Bearer {}", server::KEY)
    );
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].headers["authorization"],
        format!("Bearer {}", server::KEY)
    );
    assert_eq!(
        requests[1].headers["authorization"],
        format!("Bearer {renewed}")
    );
    let public = serde_json::to_string(&page).unwrap();
    assert!(!public.contains(server::KEY) && !public.contains(renewed));
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_partial_delivery_without_replay() {
    for remote in [false, true] {
        let server = Server::transport(ModelApi::Responses, Reply::Text, Mode::Partial).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Recover the unfinished generation").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Failed);
        assert_eq!(completion::response(&page), "原");
        assert!(super::retry::attempts(&page).is_empty());
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].body["type"], "response.create");
        assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_discards_the_socket() {
    for remote in [false, true] {
        let closed = Arc::new(tokio::sync::Notify::new());
        let server = Server::transport(
            ModelApi::Responses,
            Reply::Wait(closed.clone()),
            Mode::Accept,
        )
        .await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        for count in 1..=2 {
            let turn = fixture.submit("Wait for cancellation").await;
            tokio::time::timeout(Duration::from_secs(10), async {
                while server.requests.lock().unwrap().len() < count {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            execute(&fixture.client, Command::StopTurn { turn }).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
            tokio::time::timeout(Duration::from_secs(10), closed.notified())
                .await
                .unwrap();
        }
        assert_eq!(server.upgrades.lock().unwrap().len(), 2);
        assert_eq!(server.closed.load(Ordering::SeqCst), 2);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn resends_missing_continuation() {
    for remote in [false, true] {
        let server =
            Server::transport(ModelApi::Responses, Reply::Tool, Mode::MissingContinuation).await;
        let fixture =
            Fixture::packages(remote, ModelApi::Responses, &server.endpoint, &["files"]).await;
        std::fs::write(fixture.root.join("native.txt"), "工具内容 🙂").unwrap();
        let turn = fixture.submit("Read native.txt").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(completion::response(&page), server::ANSWER);
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolResult { .. }))
                .count(),
            1
        );
        assert!(super::retry::attempts(&page).is_empty());
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        assert_eq!(server.upgrades.lock().unwrap().len(), 1);
        assert_eq!(requests[1].body["previous_response_id"], "native-response");
        assert!(requests[2].body.get("previous_response_id").is_none());
        let original = requests[0].body["input"].as_array().unwrap();
        let full = requests[2].body["input"].as_array().unwrap();
        assert!(full.starts_with(original));
        assert!(full.iter().any(|item| item["type"] == "reasoning"
            && item["encrypted_content"] == "fixture-encrypted-reasoning"));
        assert!(
            full.iter()
                .any(|item| item["type"] == "function_call_output")
        );
        assert!(
            requests
                .iter()
                .all(|request| request.body["type"] == "response.create")
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn classifies_acknowledged_timeouts() {
    for remote in [false, true] {
        let server = Server::transport(ModelApi::Responses, Reply::Text, Mode::AckTimeout).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        fixture
            .node
            .set_model_transport_fixture(adk_model::openai::WsOptions {
                first_event_timeout: Duration::from_millis(200),
                read_timeout: Duration::from_millis(50),
                ..Default::default()
            });
        let turn = fixture
            .submit("Wait after the provider accepts the request")
            .await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Failed);
        assert!(super::retry::attempts(&page).is_empty());
        assert_eq!(completion::response(&page), "");
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].body["type"], "response.create");
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn expires_connections() {
    for remote in [false, true] {
        for idle in [false, true] {
            let start = std::time::Instant::now();
            let server = Server::transport(ModelApi::Responses, Reply::Text, Mode::Accept).await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            let lifetime = Duration::from_secs(1);
            let long = Duration::from_secs(60);
            fixture
                .node
                .set_model_transport_fixture(adk_model::openai::WsOptions {
                    idle_timeout: if idle { lifetime } else { long },
                    max_age: if idle { long } else { lifetime },
                    ..Default::default()
                });
            let mut completed = Vec::new();
            for index in 0..2 {
                let turn = fixture.submit("Use the session connection").await;
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                assert!(super::retry::attempts(&page).is_empty());
                completed.push((
                    server.upgrades.lock().unwrap().len(),
                    server
                        .requests
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|request| request.body["type"] == "response.create")
                        .count(),
                ));
                assert_eq!(
                    completed[index],
                    (index + 1, index + 1),
                    "remote={remote}, idle={idle}, completed=(handshakes,ws_creates) {completed:?}"
                );
                if index == 0 {
                    tokio::time::sleep(lifetime + Duration::from_millis(50)).await;
                }
            }
            let upgrades = server.upgrades.lock().unwrap().clone();
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(
                upgrades.len(),
                2,
                "remote={remote}, idle={idle}, handshakes={:?}, wire={:?}, completed=(handshakes,ws_creates) {completed:?}",
                upgrades
                    .iter()
                    .map(|request| request.received.duration_since(start))
                    .collect::<Vec<_>>(),
                requests
                    .iter()
                    .map(|request| (
                        request.received.duration_since(start),
                        request.body["type"].as_str().unwrap_or("http")
                    ))
                    .collect::<Vec<_>>()
            );
            assert_eq!(requests.len(), 2);
            assert!(
                requests
                    .iter()
                    .all(|request| request.body["type"] == "response.create"
                        && request.body.get("previous_response_id").is_none())
            );
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn remembers_repeated_refusals() {
    for remote in [false, true] {
        let start = std::time::Instant::now();
        let server = Server::transport(
            ModelApi::Responses,
            Reply::Text,
            Mode::RefuseAfter {
                accepted: 1,
                status: 426,
            },
        )
        .await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let lifetime = Duration::from_secs(1);
        fixture
            .node
            .set_model_transport_fixture(adk_model::openai::WsOptions {
                idle_timeout: lifetime,
                ..Default::default()
            });
        let mut completed = Vec::new();
        for (index, expected) in [1, 2, 2, 2].into_iter().enumerate() {
            let turn = fixture.submit("Use the session capability").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert_eq!(
                super::retry::attempts(&page),
                if index == 0 { vec![] } else { vec![1] }
            );
            completed.push((
                server.upgrades.lock().unwrap().len(),
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.body["type"] == "response.create")
                    .count(),
            ));
            assert_eq!(
                completed[index],
                (expected, 1),
                "remote={remote}, completed=(handshakes,ws_creates) {completed:?}"
            );
            if index < 3 {
                tokio::time::sleep(lifetime + Duration::from_millis(50)).await;
            }
        }
        // A definitive refusal switches this session without repeated probing.
        let upgrades = server.upgrades.lock().unwrap().clone();
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(
            upgrades.len(),
            2,
            "remote={remote}, handshakes={:?}, wire={:?}, completed=(handshakes,ws_creates) {completed:?}",
            upgrades
                .iter()
                .map(|request| request.received.duration_since(start))
                .collect::<Vec<_>>(),
            requests
                .iter()
                .map(|request| (
                    request.received.duration_since(start),
                    request.body["type"].as_str().unwrap_or("http")
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(requests.len(), 4);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.body["type"] == "response.create")
                .count(),
            1
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn falls_back_before_provider_acceptance() {
    for remote in [false, true] {
        let server = Server::transport(ModelApi::Responses, Reply::Text, Mode::FirstTimeout).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        fixture
            .node
            .set_model_transport_fixture(adk_model::openai::WsOptions {
                first_event_timeout: Duration::from_millis(50),
                read_timeout: Duration::from_secs(1),
                ..Default::default()
            });
        let turn = fixture
            .submit("Use the fallback before any provider event")
            .await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(completion::response(&page), server::ANSWER);
        assert_eq!(super::retry::attempts(&page), [1]);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].body["type"], "response.create");
        assert!(requests[1].body.get("type").is_none());
        let mut full = requests[1].body.clone();
        let fields = full.as_object_mut().unwrap();
        fields.remove("stream");
        fields.remove("background");
        fields.insert("type".into(), serde_json::json!("response.create"));
        assert_eq!(requests[0].body, full);
        let upgrades = server.upgrades.lock().unwrap().clone();
        assert_eq!(upgrades.len(), 1);
        assert_eq!(
            upgrades[0].headers["authorization"],
            requests[1].headers["authorization"]
        );
        assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
