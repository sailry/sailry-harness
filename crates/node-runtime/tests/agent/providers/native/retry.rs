use super::*;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use server::{Ending, Script};

const DISCARDED: &str = "Discarded draft";
const RECOVERED: &str = "Recovered answer";
const FAILURE: &str = "Upstream temporarily unavailable";

fn http(fixture: &Fixture) {
    fixture
        .node
        .set_model_transport_fixture(adk_model::openai::WsOptions {
            enabled: Arc::new(|| false),
            ..Default::default()
        });
}

async fn session_policy(fixture: &mut Fixture) {
    http(fixture);
    let mut provider = fixture.provider.clone();
    provider.request_max_retries = Some(0);
    let Output::Provider(provider) = execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    fixture.provider = provider;
}

async fn fixture(remote: bool, api: ModelApi, endpoint: &str) -> Fixture {
    let mut fixture = Fixture::empty(remote, api, endpoint).await;
    session_policy(&mut fixture).await;
    fixture
}

fn rejected(status: u16, code: Option<&str>, kind: Option<&str>) -> Script {
    let body = json!({"error":{"code":code,"type":kind,"message":FAILURE}});
    Script { bytes: format!("HTTP/1.1 {status} Failure\r\nContent-Type: application/json\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{body}").into_bytes(), ending: Ending::Close }
}

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
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nRetry-After: 0\r\n{framing}Connection: close\r\n\r\n"
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

pub(super) fn attempts(page: &Page) -> Vec<u64> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::Resource(value) if value["type"] == "model_retry" => {
                assert_eq!(value["limit"], 5);
                let error = value["error"].as_str().expect("retry error text expected");
                assert!(!error.is_empty());
                assert!(!error.contains(server::KEY));
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
            "HTTP/1.1 503 Unavailable\r\nContent-Type: application/json\r\nRetry-After: {retry_after}\r\nConnection: close\r\n\r\n{{\"error\":{{\"message\":\"{FAILURE} {}\",\"type\":\"server_error\"}}}}", server::KEY
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
            let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
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
            assert!(serialized.contains(FAILURE));
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
        let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
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
async fn ignores_invalid_server_delays() {
    for remote in [false, true] {
        let server = Server::start(
            ModelApi::Responses,
            Reply::Scripted(Arc::new(vec![
                unavailable("not-a-delay"),
                Script {
                    bytes: wire(&[completed()], true, true),
                    ending: Ending::Close,
                },
            ])),
        )
        .await;
        let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Ignore an invalid retry delay").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(completion::response(&page), RECOVERED);
        assert_eq!(attempts(&page), [1]);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

async fn recover(chunked: bool, empty: bool) {
    for remote in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let final_events = if empty {
            let mut complete = completed();
            complete["response"]["output"] = json!([]);
            vec![complete]
        } else {
            vec![
                delta("Recovered"),
                json!({"type":"response.output_text.done", "sequence_number":2,
                    "item_id":"retry-answer", "output_index":0, "content_index":0,
                    "text":RECOVERED, "logprobs":[]}),
                completed(),
            ]
        };
        let scripts = vec![
            Script {
                bytes: wire(
                    &[
                        json!({"type":"response.reasoning_summary_text.delta", "sequence_number":0,
                            "item_id":"discarded-reasoning", "output_index":0, "summary_index":0,
                            "delta":"Discarded thinking"}),
                        delta(DISCARDED),
                        json!({"type":"response.output_item.added", "sequence_number":2,
                            "output_index":1,"item":{"type":"function_call","id":"discarded-item",
                                "call_id":"discarded-call","name":"discarded_tool",
                                "arguments":"","status":"in_progress"}}),
                        json!({"type":"response.function_call_arguments.delta", "sequence_number":3,
                            "output_index":1,"item_id":"discarded-item","delta":"{\"discarded\":"}),
                    ],
                    chunked,
                    false,
                ),
                ending: Ending::Release(release.clone()),
            },
            Script {
                bytes: wire(&final_events, true, true),
                ending: Ending::Close,
            },
        ];
        let server = Server::start(ModelApi::Responses, Reply::Scripted(Arc::new(scripts))).await;
        let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
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
                            draft.turn == turn
                                && draft.parts.contains(&Part::Text(DISCARDED.into()))
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
            let run = page.runs.last().unwrap();
            assert_eq!(
                run.status,
                if empty {
                    Status::Failed
                } else {
                    Status::Completed
                },
                "{page:?}"
            );
            if empty {
                assert_eq!(
                    run.error.as_ref().unwrap().message,
                    "Model returned no answer"
                );
            }
            assert_eq!(
                completion::response(&page),
                if empty { "" } else { RECOVERED }
            );
            assert!(
                !page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| {
                        matches!(
                            part,
                            Part::Thinking(_) | Part::ToolCall { .. } | Part::ToolResult { .. }
                        )
                    })
            );
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| {
                        entry.author == "assistant"
                            && entry.parts.iter().any(|part| matches!(part, Part::Text(_)))
                    })
                    .count(),
                usize::from(!empty)
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
    recover(true, false).await;
}

#[tokio::test]
async fn recovers_premature_end() {
    recover(false, false).await;
}

#[tokio::test]
async fn clears_discarded_output_after_empty_recovery() {
    recover(true, true).await;
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
        let mut first = chat_script(event);
        first.ending = Ending::Release(release.clone());
        let server = Server::start(
            ModelApi::ChatCompletions,
            Reply::Scripted(Arc::new(vec![first, rejected(400, None, None)])),
        )
        .await;
        let mut fixture = Fixture::packages(
            remote,
            ModelApi::ChatCompletions,
            &server.endpoint,
            &["files"],
        )
        .await;
        session_policy(&mut fixture).await;
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let turn = fixture.submit("Write once.txt").await;
        // DONE commits the tool and releases its HTTP stream, even while the server
        // retains that socket. A later failed model call cannot repeat the write.
        let page = tokio::time::timeout(
            Duration::from_secs(10),
            finished(&fixture.client, fixture.session.id, turn),
        )
        .await
        .expect("completed tool must not wait for HTTP EOF");
        release.notify_one();
        assert_eq!(page.runs.last().unwrap().status, Status::Failed);
        assert_eq!(
            page.runs.last().unwrap().error.as_ref().unwrap().code,
            ErrorCode::Unavailable
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert!(attempts(&page).is_empty());
        for result in [false, true] {
            let ids: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::ToolCall { id, .. } if !result => id.as_deref(),
                    Part::ToolResult { id, .. } if result => id.as_deref(),
                    _ => None,
                })
                .collect();
            assert_eq!(ids, ["write-once"]);
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("once.txt")).unwrap(),
            "once"
        );
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
        let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
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
async fn shares_opening_and_partial_retry_budget() {
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
            Script {
                bytes: wire(&[delta("Uncommitted recovery")], true, false),
                ending: Ending::Close,
            },
            Script {
                bytes: wire(&[completed()], true, true),
                ending: Ending::Close,
            },
        ];
        let server = Server::start(ModelApi::Responses, Reply::Scripted(Arc::new(scripts))).await;
        let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Bound opening and streaming retries").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(attempts(&page), vec![1, 2, 3, 4]);
        let errors: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::Resource(value) if value["type"] == "model_retry" => value["error"].as_str(),
                _ => None,
            })
            .collect();
        assert_eq!(errors.len(), 4);
        assert!(errors[0].contains("fixture unavailable"));
        assert_eq!(errors[0], errors[1]);
        assert_ne!(errors[1], errors[2]);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 5);
            assert!(
                requests
                    .iter()
                    .all(|request| request.body == requests[0].body)
            );
        }
        assert_eq!(completion::response(&page), RECOVERED);
        unique_entries(&page);
        let continuation = fixture.submit("Continue from the saved answer").await;
        let continued = finished(&fixture.client, fixture.session.id, continuation).await;
        assert_eq!(continued.runs.last().unwrap().status, Status::Completed);
        assert!(completion::response(&continued).ends_with(RECOVERED));
        let sent = {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 6);
            requests.last().unwrap().body.to_string()
        };
        assert!(!sent.contains(DISCARDED));
        assert!(sent.contains(RECOVERED));
        assert!(sent.contains("Continue from the saved answer"));
        assert!(!sent.contains("Uncommitted recovery"));
        assert!(!sent.contains("fixture unavailable"));
        assert!(!sent.contains("model.openai_responses.unavailable"));
        assert!(
            page.entries
                .iter()
                .all(|entry| continued.entries.contains(entry))
        );
        unique_entries(&continued);
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
        let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
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

#[tokio::test]
async fn bounds_retries_before_any_output() {
    for remote in [false, true] {
        let server = Server::start(
            ModelApi::Responses,
            Reply::Scripted(Arc::new(vec![unavailable("0")])),
        )
        .await;
        let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Bound retries without model output").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Failed);
        assert_eq!(attempts(&page), [1, 2, 3, 4, 5]);
        assert_eq!(server.requests.lock().unwrap().len(), 6);
        unique_entries(&page);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

fn chat_script(event: Value) -> Script {
    Script {
        bytes: format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {event}\n\ndata: [DONE]\n\n").into_bytes(),
        ending: Ending::Close,
    }
}

#[tokio::test]
async fn continues_without_repeating_completed_tools() {
    for remote in [false, true] {
        let tool = crate::agent_support::plugin_tool("files", "write_file");
        let first = chat_script(
            json!({"id":"write-response", "object":"chat.completion.chunk",
            "created":1, "model":"fixture-a", "choices":[{"index":0,
                "delta":{"tool_calls":[{"index":0,"id":"write-once","type":"function",
                    "function":{"name":tool,"arguments":json!({"path":"once.txt","text":"once","expected_revision":null}).to_string()}}]},
                "finish_reason":"tool_calls"}]}),
        );
        let last = chat_script(
            json!({"id":"answer-response", "object":"chat.completion.chunk",
            "created":1, "model":"fixture-a", "choices":[{"index":0,
                "delta":{"content":RECOVERED}, "finish_reason":"stop"}]}),
        );
        let server = Server::start(
            ModelApi::ChatCompletions,
            Reply::Dynamic(Arc::new(move |index| match index {
                0 => first.clone(),
                1..=6 => unavailable("0"),
                _ => last.clone(),
            })),
        )
        .await;
        let mut fixture = Fixture::packages(
            remote,
            ModelApi::ChatCompletions,
            &server.endpoint,
            &["files"],
        )
        .await;
        session_policy(&mut fixture).await;
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let turn = fixture.submit("Write once.txt, then summarize").await;
        let failed = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(failed.runs.last().unwrap().status, Status::Failed);
        assert_eq!(server.requests.lock().unwrap().len(), 7);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("once.txt")).unwrap(),
            "once"
        );
        let results = |page: &Page| {
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolResult { .. }))
                .count()
        };
        assert_eq!(results(&failed), 1);
        let resumed = fixture.submit("Continue from the completed work").await;
        let page = finished(&fixture.client, fixture.session.id, resumed).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(completion::response(&page).ends_with(RECOVERED));
        assert_eq!(results(&page), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 8);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("once.txt")).unwrap(),
            "once"
        );
        assert!(
            failed
                .entries
                .iter()
                .all(|entry| page.entries.contains(entry))
        );
        unique_entries(&page);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn bounds_classified_response_failures() {
    for remote in [false, true] {
        for (code, retries) in [
            (Some("server_error"), 5),
            (Some("stream_timeout"), 5),
            (Some("future_backend_failure"), 5),
            (Some("subscription_sharing_usage_limit_exceeded"), 5),
            (Some("insufficient_quota"), 0),
            (None, 5),
        ] {
            let message = format!(
                "Our servers are currently overloaded. Please try again later. authorization {}",
                server::KEY
            );
            let failed = json!({"type":"response.failed","sequence_number":1,"response":{
                "id":"failure","object":"response","created_at":1,"model":"fixture-a","status":"failed","output":[],
                "error":{"code":code,"message":message,"param":format!("Bearer {}", server::KEY)},
            }});
            let server = Server::start(
                ModelApi::Responses,
                Reply::Scripted(Arc::new(vec![Script {
                    bytes: wire(&[failed], false, true),
                    ending: Ending::Close,
                }])),
            )
            .await;
            let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
            let turn = fixture.submit("Bound the response failure retries").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Failed);
            assert_eq!(attempts(&page), (1..=retries).collect::<Vec<_>>());
            let failure = page.runs.last().unwrap().error.as_ref().unwrap();
            assert!(
                failure
                    .message
                    .contains("model.openai_responses.response_failed")
            );
            assert!(
                failure
                    .message
                    .contains("Our servers are currently overloaded")
            );
            assert!(failure.message.contains("[redacted]"));
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), retries as usize + 1);
                assert!(
                    requests
                        .iter()
                        .all(|request| request.body == requests[0].body)
                );
            }
            assert!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .all(|part| !matches!(part, Part::ToolCall { .. } | Part::ToolResult { .. }))
            );
            unique_entries(&page);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn separates_raw_http_and_session_recovery() {
    for remote in [false, true] {
        let success = Script {
            bytes: wire(&[completed()], true, true),
            ending: Ending::Close,
        };
        let mut scripts = vec![unavailable("0"); 4];
        scripts.push(success);
        let server = Server::start(ModelApi::Responses, Reply::Scripted(Arc::new(scripts))).await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        http(&fixture);
        let turn = fixture.submit("Recover inside the request layer").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(attempts(&page).is_empty());
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 5);
        assert!(
            requests
                .iter()
                .all(|request| request.body == requests[0].body)
        );
        drop(requests);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn maps_http_failures_before_session_recovery() {
    for remote in [false, true] {
        for (status, code, kind, retry) in [
            (401, None, None, true),
            (403, None, None, true),
            (400, None, None, false),
            (429, None, None, true),
            (429, None, Some("insufficient_quota"), false),
            (
                429,
                Some("subscription_sharing_usage_limit_exceeded"),
                None,
                true,
            ),
        ] {
            let server = Server::start(
                ModelApi::Responses,
                Reply::Scripted(Arc::new(vec![
                    rejected(status, code, kind),
                    Script {
                        bytes: wire(&[completed()], true, true),
                        ending: Ending::Close,
                    },
                ])),
            )
            .await;
            let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
            let turn = fixture.submit("Apply the captured recovery policy").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                if retry {
                    Status::Completed
                } else {
                    Status::Failed
                }
            );
            assert_eq!(attempts(&page), if retry { vec![1] } else { vec![] });
            assert_eq!(
                server.requests.lock().unwrap().len(),
                if retry { 2 } else { 1 }
            );
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn ignores_standalone_errors_before_completion() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Responses, Reply::Scripted(Arc::new(vec![Script {
            bytes: wire(&[json!({"type":"error","error":{"code":"insufficient_quota","message":"ignored standalone diagnostic"}}), completed()], true, true),
            ending: Ending::Close,
        }]))).await;
        let fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Consume the authoritative completion").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(attempts(&page).is_empty());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancels_persistent_connection_wait() {
    for remote in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        drop(listener);
        let fixture = fixture(remote, ModelApi::Responses, &endpoint).await;
        let turn = fixture.submit("Wait for the unavailable network").await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                let retry =
                    page.entries.iter().flat_map(|entry| &entry.parts).find_map(
                        |part| match part {
                            Part::Resource(value) if value["type"] == "model_retry" => Some(value),
                            _ => None,
                        },
                    );
                if let Some(retry) = retry {
                    assert_eq!(retry["attempt"], 1);
                    assert!(retry["limit"].is_null());
                    assert_eq!(retry["transport"], "http");
                    assert!(retry["delay_ms"].as_u64().unwrap() <= 5000);
                    assert_eq!(page.runs.last().unwrap().status, Status::Running);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("persistent retry notification");
        execute(&fixture.client, Command::StopTurn { turn }).await;
        let page = tokio::time::timeout(
            Duration::from_secs(3),
            finished(&fixture.client, fixture.session.id, turn),
        )
        .await
        .unwrap();
        assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn retains_queued_recovery_overrides() {
    for remote in [false, true] {
        let success = Script {
            bytes: wire(&[completed()], true, true),
            ending: Ending::Close,
        };
        let server = Server::start(
            ModelApi::Responses,
            Reply::Scripted(Arc::new(vec![unavailable("0"), success, unavailable("0")])),
        )
        .await;
        let mut fixture = fixture(remote, ModelApi::Responses, &server.endpoint).await;
        let mut provider = fixture.provider.clone();
        provider.stream_max_retries = Some(1);
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let Output::QueuedTurn(queued) = execute(
            &fixture.client,
            Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Use the captured recovery limit".into(),
            },
        )
        .await
        else {
            panic!("queued turn expected")
        };
        let mut changed = provider;
        changed.stream_max_retries = Some(0);
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: changed.revision,
                provider: changed,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        fixture.provider = provider;
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let retry = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .find_map(|part| match part {
                Part::Resource(value) if value["type"] == "model_retry" => Some(value),
                _ => None,
            })
            .expect("captured session retry");
        assert_eq!(retry["attempt"], 1);
        assert_eq!(retry["limit"], 1);
        let next = fixture.submit("Use the new recovery setting").await;
        let page = finished(&fixture.client, fixture.session.id, next).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Failed);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
