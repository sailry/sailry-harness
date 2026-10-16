use super::*;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use server::{Ending, Script};

const DISCARDED: &str = "Discarded draft";
const RECOVERED: &str = "Recovered answer";

fn delta(text: &str) -> Value {
    json!({"type":"response.output_text.delta", "sequence_number":1,
        "item_id":"retry-answer", "output_index":0, "content_index":0,
        "delta":text, "logprobs":[]})
}

fn completed() -> Value {
    json!({"type":"response.completed", "sequence_number":3,
        "response":{"id":"retry-response", "object":"response", "created_at":1,
            "model":"fixture-a", "status":"completed",
            "output":[{"type":"message", "id":"retry-answer", "role":"assistant",
                "status":"completed", "content":[{"type":"output_text",
                    "text":RECOVERED, "annotations":[]}]}]}})
}

fn wire(events: &[Value], chunked: bool, complete: bool) -> Vec<u8> {
    let framing = if chunked {
        "Transfer-Encoding: chunked\r\n"
    } else {
        ""
    };
    let mut bytes = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n{framing}Connection: close\r\n\r\n"
    );
    for event in events {
        let event = format!("data: {event}\n\n");
        if chunked {
            bytes.push_str(&format!("{:x}\r\n{event}\r\n", event.len()));
        } else {
            bytes.push_str(&event);
        }
    }
    if chunked && complete {
        bytes.push_str("0\r\n\r\n");
    }
    bytes.into_bytes()
}

fn attempts(page: &Page) -> Vec<u64> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::Resource(value) if value["type"] == "model_retry" => {
                assert_eq!(value["limit"], 5);
                value["attempt"].as_u64()
            }
            _ => None,
        })
        .collect()
}

fn unique_entries(page: &Page) {
    let ids: std::collections::BTreeSet<_> = page.entries.iter().map(|entry| &entry.id).collect();
    assert_eq!(ids.len(), page.entries.len());
}

fn unavailable(retry_after: &str) -> Script {
    Script {
        bytes: format!(
            "HTTP/1.1 503 Unavailable\r\nContent-Type: application/json\r\nRetry-After: {retry_after}\r\nConnection: close\r\n\r\n{{\"error\":{{\"message\":\"private provider failure\",\"type\":\"server_error\"}}}}"
        ).into_bytes(),
        ending: Ending::Close,
    }
}

#[tokio::test]
async fn honors_server_retry_delays() {
    for remote in [false, true] {
        for date in [false, true] {
            let success = Script {
                bytes: wire(&[completed()], true, true),
                ending: Ending::Close,
            };
            let deadline = Arc::new(std::sync::Mutex::new(None));
            let reply = if date {
                let deadline = deadline.clone();
                Reply::Dynamic(Arc::new(move |index| {
                    if index > 0 {
                        return success.clone();
                    }
                    let due =
                        chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp() + 4, 0)
                            .unwrap();
                    *deadline.lock().unwrap() = Some(std::time::SystemTime::from(due));
                    unavailable(&due.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
                }))
            } else {
                Reply::Scripted(Arc::new(vec![unavailable("2"), success]))
            };
            let server = Server::start(ModelApi::Responses, reply).await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            let turn = fixture.submit("Respect the server retry delay").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert_eq!(completion::response(&page), RECOVERED);
            assert_eq!(attempts(&page), vec![1]);
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), 2);
                assert_eq!(requests[0].body, requests[1].body);
                if date {
                    let deadline = deadline.lock().unwrap().expect("date reply was sent");
                    assert!(
                        requests[0].received_at < deadline,
                        "date fixture must still be in the future"
                    );
                    assert!(requests[1].received_at >= deadline);
                } else {
                    assert!(
                        requests[1].received.duration_since(requests[0].received)
                            >= Duration::from_secs(2)
                    );
                }
            }
            let serialized = serde_json::to_string(&page).unwrap();
            assert!(!serialized.contains("private provider failure"));
            assert!(!serialized.contains(server::KEY));
            unique_entries(&page);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn cancels_a_hinted_wait_without_replay() {
    for remote in [false, true] {
        let server = Server::start(
            ModelApi::Responses,
            Reply::Scripted(Arc::new(vec![unavailable("30")])),
        )
        .await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Cancel the server retry wait").await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                if attempts(&page) == vec![1] {
                    assert_eq!(page.runs.last().unwrap().status, Status::Running);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("hinted retry must be admitted before stopping");
        execute(&fixture.client, Command::StopTurn { turn }).await;
        let page = tokio::time::timeout(
            Duration::from_secs(3),
            finished(&fixture.client, fixture.session.id, turn),
        )
        .await
        .expect("cancellation must not wait for the server delay");
        assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
        assert_eq!(attempts(&page), vec![1]);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_unrepresentable_server_delays() {
    for remote in [false, true] {
        let server = Server::start(
            ModelApi::Responses,
            Reply::Scripted(Arc::new(vec![unavailable("18446744073709551616")])),
        )
        .await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Retain the unavailable outcome").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Failed);
        assert_eq!(
            run.error.as_ref().unwrap().message,
            "Agent execution failed (model.openai_responses.unavailable); HTTP 503; unavailable"
        );
        assert!(attempts(&page).is_empty());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

async fn recover(chunked: bool) {
    for remote in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let scripts = vec![
            Script {
                bytes: wire(&[delta(DISCARDED)], chunked, false),
                ending: Ending::Release(release.clone()),
            },
            Script {
                bytes: wire(
                    &[
                        delta("Recovered"),
                        json!({"type":"response.output_text.done", "sequence_number":2,
                            "item_id":"retry-answer", "output_index":0, "content_index":0,
                            "text":RECOVERED, "logprobs":[]}),
                        completed(),
                    ],
                    true,
                    true,
                ),
                ending: Ending::Close,
            },
        ];
        let server = Server::start(ModelApi::Responses, Reply::Scripted(Arc::new(scripts))).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let (updates, mut observed) = tokio::sync::watch::channel(View::default());
        let stop = CancellationToken::new();
        let (_requests, requests) = tokio::sync::mpsc::channel(1);
        let watcher =
            fixture
                .client
                .watch_conversation(fixture.session.id, updates, stop.clone(), requests);
        let verify = async {
            let turn = fixture.submit("Recover the interrupted answer").await;
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if observed.borrow().snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.drafts.iter().any(|draft| {
                            draft.turn == turn && draft.parts == vec![Part::Text(DISCARDED.into())]
                        })
                    }) {
                        break;
                    }
                    observed.changed().await.unwrap();
                }
            })
            .await
            .expect("first attempt must publish before disconnection");
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            release.notify_one();
            let page = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if let Some(snapshot) = observed.borrow().snapshot.as_ref() {
                        unique_entries(&snapshot.page);
                        for draft in &snapshot.drafts {
                            let text: String = draft
                                .parts
                                .iter()
                                .filter_map(|part| match part {
                                    Part::Text(text) => Some(text.as_str()),
                                    _ => None,
                                })
                                .collect();
                            assert!(!text.contains("Discarded draftRecovered"), "{text}");
                            assert!(!text.contains("RecoveredRecovered"), "{text}");
                        }
                        if snapshot.page.runs.iter().any(|run| {
                            run.turn == turn
                                && matches!(run.status, Status::Completed | Status::Failed)
                        }) {
                            assert!(snapshot.drafts.is_empty());
                            break snapshot.page.as_ref().clone();
                        }
                    }
                    observed.changed().await.unwrap();
                }
            })
            .await
            .expect("recovered answer deadline");
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            assert_eq!(completion::response(&page), RECOVERED);
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| {
                        entry.author == "assistant"
                            && entry.parts.iter().any(|part| matches!(part, Part::Text(_)))
                    })
                    .count(),
                1
            );
            assert_eq!(attempts(&page), vec![1]);
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].body, requests[1].body);
            drop(requests);
            stop.cancel();
        };
        let (result, ()) = tokio::join!(watcher, verify);
        result.unwrap();
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn recovers_partial_disconnect() {
    recover(true).await;
}

#[tokio::test]
async fn recovers_premature_end() {
    recover(false).await;
}

#[tokio::test]
async fn preserves_committed_tools_without_replay() {
    for remote in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let tool = crate::agent_support::plugin_tool("files", "write_file");
        let event = json!({"id":"committed-call", "object":"chat.completion.chunk",
            "created":1, "model":"fixture-a", "choices":[{"index":0,
                "delta":{"tool_calls":[{"index":0, "id":"write-once", "type":"function",
                    "function":{"name":tool, "arguments":json!({"path":"once.txt", "text":"once", "expected_revision":null}).to_string()}}]},
                "finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":8,"completion_tokens":3,"total_tokens":11}});
        let server = Server::start(
            ModelApi::ChatCompletions,
            Reply::Scripted(Arc::new(vec![Script {
                bytes: wire(&[event], true, false),
                ending: Ending::Release(release.clone()),
            }])),
        )
        .await;
        let fixture = Fixture::packages(
            remote,
            ModelApi::ChatCompletions,
            &server.endpoint,
            &["files"],
        )
        .await;
        let turn = fixture.submit("Write once.txt").await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                if page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part, Part::ToolCall { .. }))
                {
                    assert_eq!(page.runs.last().unwrap().status, Status::Running);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("full tool call must commit before disconnect");
        release.notify_one();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Failed);
        assert_eq!(
            page.runs.last().unwrap().error.as_ref().unwrap().code,
            ErrorCode::Unavailable
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert!(attempts(&page).is_empty());
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolCall { .. }))
                .count(),
            1
        );
        assert!(
            !page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::ToolResult { .. }))
        );
        assert!(!fixture.root.join("once.txt").exists());
        unique_entries(&page);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_committed_answers_without_replay() {
    for remote in [false, true] {
        // The complete event is authoritative even if HTTP framing ends abruptly.
        let server = Server::start(
            ModelApi::Responses,
            Reply::Scripted(Arc::new(vec![Script {
                bytes: wire(&[completed()], true, false),
                ending: Ending::Close,
            }])),
        )
        .await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Answer once").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(completion::response(&page), RECOVERED);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert!(attempts(&page).is_empty());
        unique_entries(&page);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn shares_opening_and_stream_retry_budget() {
    for remote in [false, true] {
        let failure = Script {
            bytes: b"HTTP/1.1 503 Unavailable\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"fixture unavailable\",\"type\":\"server_error\"}}".to_vec(),
            ending: Ending::Close,
        };
        let scripts = vec![
            failure.clone(),
            failure,
            Script {
                bytes: wire(&[delta(DISCARDED)], true, false),
                ending: Ending::Close,
            },
        ];
        let server = Server::start(ModelApi::Responses, Reply::Scripted(Arc::new(scripts))).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Bound opening and streaming retries").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Failed);
        assert_eq!(
            page.runs.last().unwrap().error.as_ref().unwrap().code,
            ErrorCode::Unavailable
        );
        assert_eq!(attempts(&page), vec![1, 2, 3, 4, 5]);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 6);
            assert!(
                requests
                    .iter()
                    .all(|request| request.body == requests[0].body)
            );
        }
        assert_eq!(completion::response(&page), DISCARDED);
        unique_entries(&page);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancels_recovery_without_another_request() {
    for remote in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let closed = Arc::new(tokio::sync::Notify::new());
        let scripts = vec![
            Script {
                bytes: wire(&[delta(DISCARDED)], true, false),
                ending: Ending::Release(release.clone()),
            },
            Script {
                bytes: wire(&[delta("Recovering")], true, false),
                ending: Ending::Closed(closed.clone()),
            },
        ];
        let server = Server::start(ModelApi::Responses, Reply::Scripted(Arc::new(scripts))).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Cancel recovery").await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while server.requests.lock().unwrap().len() != 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("initial request deadline");
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(10), async {
            while server.requests.lock().unwrap().len() != 2 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("recovery request deadline");
        execute(&fixture.client, Command::StopTurn { turn }).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
        tokio::time::timeout(Duration::from_secs(10), closed.notified())
            .await
            .unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert_eq!(attempts(&page), vec![1]);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
