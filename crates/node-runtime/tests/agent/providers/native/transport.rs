use super::*;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;
use server::Mode;
use std::sync::atomic::Ordering;

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
async fn remembers_confirmed_refusals() {
    for mode in [
        Mode::Refuse(404),
        Mode::Refuse(426),
        Mode::Refuse(401),
        Mode::Close(1008),
        Mode::Close(1013),
        Mode::Limit,
    ] {
        let server = Server::transport(ModelApi::Responses, Reply::Text, mode).await;
        let fixture = Fixture::empty(false, ModelApi::Responses, &server.endpoint).await;
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
            assert!(super::retry::attempts(&page).is_empty());
        }
        let expected = if matches!(mode, Mode::Close(1013) | Mode::Limit) {
            2
        } else {
            1
        };
        assert_eq!(server.upgrades.lock().unwrap().len(), expected);
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

#[tokio::test]
async fn keeps_authentication_failures_retryable() {
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
    assert!(super::retry::attempts(&page).is_empty());
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
    assert_eq!(upgrades.len(), 2);
    assert_eq!(
        upgrades[0].headers["authorization"],
        format!("Bearer {}", server::KEY)
    );
    assert_eq!(
        upgrades[1].headers["authorization"],
        format!("Bearer {renewed}")
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
async fn recovers_without_transport_replay() {
    for remote in [false, true] {
        let server = Server::transport(ModelApi::Responses, Reply::Text, Mode::Partial).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Recover the answer").await;
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
                .all(|request| request.body["type"] == "response.create"
                    && request.body.get("previous_response_id").is_none())
        );
        assert_eq!(requests[0].body["input"], requests[1].body["input"]);
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
