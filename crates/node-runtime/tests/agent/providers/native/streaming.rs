use super::*;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;
use serde_json::json;

#[tokio::test]
async fn publishes_before_completion() {
    for remote in [false, true] {
        for (content_type, ending) in [
            (Some("text/event-stream"), "\n"),
            (None, "\n"),
            (Some("application/octet-stream"), "\r\n"),
            (Some("application/json"), "\r"),
        ] {
            let gates: Vec<_> = (0..3)
                .map(|_| Arc::new(tokio::sync::Notify::new()))
                .collect();
            let server =
                crate::agent_support::Server::staged_responses(gates.clone(), content_type, ending)
                    .await;
            let fixture = Fixture::new(remote, ModelApi::Responses, &server.endpoint).await;
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
                stop.cancel();
            };
            let (result, ()) = tokio::join!(watcher, verify);
            result.unwrap();
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn preserves_explicit_json_responses() {
    use adk_core::{Content, Llm, LlmRequest};
    use adk_model::openai::{OpenAIResponsesClient, OpenAIResponsesConfig};
    use futures::StreamExt;
    let response = json!({"id":"json-response", "object":"response", "created_at":1, "model":"fixture-a",
        "status":"completed", "output":[{"type":"message", "id":"json-answer", "role":"assistant", "status":"completed",
            "content":[{"type":"output_text", "text":"Explicit JSON answer", "annotations":[]}]}]});
    let body = response.to_string();
    let raw = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let server = Server::start(ModelApi::Responses, Reply::Raw(Arc::new(raw.into_bytes()))).await;
    let model = OpenAIResponsesClient::new(
        OpenAIResponsesConfig::new(server::KEY, "fixture-a")
            .with_base_url(&server.endpoint)
            .with_open_responses_mode(true),
    )
    .unwrap();
    let mut stream = model
        .generate_content(
            LlmRequest::new(
                "fixture-a",
                vec![Content::new("user").with_text("Answer directly")],
            ),
            false,
        )
        .await
        .unwrap();
    let response = stream.next().await.unwrap().unwrap();
    assert!(!response.partial);
    assert!(response.turn_complete);
    assert_eq!(
        response.content.unwrap().parts[0].text(),
        Some("Explicit JSON answer")
    );
    assert!(stream.next().await.is_none());
    assert_ne!(server.requests.lock().unwrap()[0].body["stream"], true);
}
