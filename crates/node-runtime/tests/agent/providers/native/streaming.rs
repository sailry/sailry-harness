use super::*;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;
use serde_json::json;

#[tokio::test]
async fn distinguishes_eof_from_transport_failure() {
    for remote in [false, true] {
        for api in [
            ModelApi::ChatCompletions,
            ModelApi::Responses,
            ModelApi::Anthropic,
            ModelApi::Gemini,
        ] {
            for broken_transport in [false, true] {
                let events = match api {
                    ModelApi::ChatCompletions => vec![json!({
                        "id":"partial","object":"chat.completion.chunk","created":1,"model":"fixture-a",
                        "choices":[{"index":0,"delta":{"content":"Partial answer"},"finish_reason":null}]
                    })],
                    ModelApi::Responses => vec![json!({
                        "type":"response.output_text.delta","sequence_number":0,"item_id":"partial",
                        "output_index":0,"content_index":0,"delta":"Partial answer","logprobs":[]
                    })],
                    ModelApi::Anthropic => vec![
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Partial answer"}}),
                    ],
                    ModelApi::Gemini => vec![json!({
                        "candidates":[{"content":{"role":"model","parts":[{"text":"Partial answer"}]}}],
                        "modelVersion":"fixture-a"
                    })],
                    _ => unreachable!(),
                };
                let body = events
                    .into_iter()
                    .map(|event| format!("data: {event}\n\n"))
                    .collect::<String>();
                let raw = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len() + if broken_transport { 100 } else { 0 },
                );
                let server = Server::start(api, Reply::Raw(Arc::new(raw.into_bytes()))).await;
                let fixture = Fixture::empty(remote, api, &server.endpoint).await;
                let turn = fixture.submit("Keep partial output on interruption").await;
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                let run = page.runs.last().unwrap();
                let (status, outcome) = if broken_transport {
                    (
                        Status::Failed,
                        sailry_protocol::conversation::observation::Outcome::Failed,
                    )
                } else {
                    (
                        Status::Completed,
                        sailry_protocol::conversation::observation::Outcome::Succeeded,
                    )
                };
                assert_eq!(
                    run.status, status,
                    "{api:?}, remote={remote}, transport={broken_transport}: {:?}",
                    page.runs
                );
                assert_eq!(run.error.is_some(), broken_transport);
                assert!(
                    completion::response(&page).contains("Partial answer"),
                    "{api:?}: {:?}",
                    page.entries
                );
                assert_eq!(
                    server.requests.lock().unwrap().len(),
                    1,
                    "partial generation must not be replayed"
                );
                let statistics =
                    crate::observations::statistics(&fixture.client, fixture.session.id).await;
                let request = statistics
                    .observations
                    .recent
                    .iter()
                    .find(|request| request.turn == turn)
                    .unwrap();
                assert_eq!(request.attempts.len(), 1);
                assert_eq!(request.attempts[0].outcome, outcome);
                fixture.controller.close().await.unwrap();
                fixture.node.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn publishes_before_completion() {
    for remote in [false, true] {
        for (content_type, ending) in [
            (Some("text/event-stream"), "\n"),
            (None, "\n"),
            (Some("application/octet-stream"), "\r\n"),
            (Some("application/json"), "\r\n"),
        ] {
            let gates: Vec<_> = (0..3)
                .map(|_| Arc::new(tokio::sync::Notify::new()))
                .collect();
            let server =
                crate::agent_support::Server::staged_responses(gates.clone(), content_type, ending)
                    .await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            let (updates, mut observed) = tokio::sync::watch::channel(View::default());
            let stop = CancellationToken::new();
            let (_requests, requests) = tokio::sync::mpsc::channel(1);
            let watcher = fixture.client.watch_conversation(
                fixture.session.id,
                updates,
                stop.clone(),
                requests,
            );
            let verify = async {
                let turn = fixture.submit("Stream the answer incrementally").await;
                for (index, expected) in ["First", "First second", "First second third"]
                    .iter()
                    .enumerate()
                {
                    tokio::time::timeout(Duration::from_secs(10), async {
                        loop {
                            if observed.borrow().snapshot.as_ref().is_some_and(|snapshot| {
                                snapshot.drafts.iter().any(|draft| {
                                    draft.turn == turn
                                        && draft.parts == vec![Part::Text((*expected).into())]
                                })
                            }) {
                                break;
                            }
                            observed.changed().await.unwrap();
                        }
                    })
                    .await
                    .expect("incremental Responses output deadline");
                    {
                        let observed = observed.borrow();
                        let snapshot = observed.snapshot.as_ref().unwrap();
                        assert!(observed.connected);
                        assert_eq!(
                            snapshot
                                .page
                                .runs
                                .iter()
                                .find(|run| run.turn == turn)
                                .unwrap()
                                .status,
                            Status::Running
                        );
                        assert!(
                            snapshot
                                .page
                                .entries
                                .iter()
                                .filter(|entry| entry.turn == turn && entry.author != "user")
                                .all(|entry| entry
                                    .parts
                                    .iter()
                                    .all(|part| !matches!(part, Part::Text(_))))
                        );
                    }
                    gates[index].notify_one();
                }
                tokio::time::timeout(Duration::from_secs(10), async {
                    loop {
                        if observed.borrow().snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot
                                .page
                                .runs
                                .iter()
                                .any(|run| run.turn == turn && run.status == Status::Completed)
                        }) {
                            break;
                        }
                        observed.changed().await.unwrap();
                    }
                })
                .await
                .expect("Responses completion deadline");
                let snapshot = observed.borrow().snapshot.clone().unwrap();
                assert!(snapshot.drafts.is_empty());
                assert_eq!(completion::response(&snapshot.page), "First second third");
                assert_eq!(server.requests.lock().unwrap().len(), 1);
                assert_eq!(server.requests.lock().unwrap()[0]["stream"], true);
                assert_eq!(
                    server.authorization.lock().unwrap().as_slice(),
                    &[Some(format!("Bearer {}", server::KEY))]
                );
                stop.cancel();
            };
            let (result, ()) = tokio::join!(watcher, verify);
            result.unwrap();
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
