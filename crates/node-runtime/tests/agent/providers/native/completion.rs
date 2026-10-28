use super::*;
use serde_json::{Value, json};

pub(super) fn response(page: &Page) -> String {
    page.entries
        .iter()
        .filter(|entry| entry.author != "user")
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn message(text: &str) -> Value {
    json!({"type":"message", "id":"answer", "role":"assistant", "status":"completed",
        "content":[{"type":"output_text", "text":text, "annotations":[]}]})
}

fn completed(output: Value) -> Value {
    json!({"type":"response.completed", "sequence_number":10,
        "response":{"id":"completed", "object":"response", "created_at":1,
        "model":"fixture-a", "status":"completed", "output":output}})
}

pub(super) async fn server(events: Vec<Value>) -> Server {
    let mut body =
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            .to_string();
    for event in events {
        body.push_str(&format!("data: {event}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    Server::start(ModelApi::Responses, Reply::Raw(Arc::new(body.into_bytes()))).await
}

#[tokio::test]
async fn retains_streamed_and_final_content() {
    let answer = message("A complete answer");
    let mut first = message("A complete");
    first["id"] = json!("first");
    let last = message(" answer");
    let mut multipart = answer.clone();
    multipart["content"] = json!([
        {"type":"output_text", "text":"A complete", "annotations":[]},
        {"type":"output_text", "text":" answer", "annotations":[]}
    ]);
    let cases = [
        vec![completed(json!([answer]))],
        vec![
            json!({"type":"response.output_text.delta", "sequence_number":1, "item_id":"answer",
                "output_index":0, "content_index":0, "delta":"A complete", "logprobs":[]}),
            completed(json!([answer])),
        ],
        vec![
            json!({"type":"response.output_item.done", "sequence_number":1,
                "output_index":0, "item":answer}),
            completed(json!([])),
        ],
        vec![
            json!({"type":"response.output_text.delta", "sequence_number":1, "item_id":"answer",
                "output_index":0, "content_index":0, "delta":"Draft", "logprobs":[]}),
            json!({"type":"response.output_text.done", "sequence_number":2, "item_id":"answer",
                "output_index":0, "content_index":0, "text":"A complete answer", "logprobs":[]}),
            completed(json!([answer])),
        ],
        vec![
            json!({"type":"response.output_text.delta", "sequence_number":1, "item_id":"answer",
                "output_index":0, "content_index":0, "delta":"Draft", "logprobs":[]}),
            json!({"type":"response.output_text.done", "sequence_number":2, "item_id":"answer",
                "output_index":0, "content_index":0, "text":"A complete answer", "logprobs":[]}),
            completed(json!([])),
        ],
        vec![completed(
            json!([{"type":"message", "id":"answer", "role":"assistant", "status":"completed",
            "content":[{"type":"refusal", "refusal":"A complete answer"}]}]),
        )],
        vec![
            json!({"type":"response.output_text.delta", "sequence_number":1, "item_id":"answer",
                "output_index":1, "content_index":0, "delta":" answer", "logprobs":[]}),
            completed(json!([first, last])),
        ],
        vec![
            json!({"type":"response.output_text.delta", "sequence_number":1, "item_id":"answer",
                "output_index":0, "content_index":1, "delta":" answer", "logprobs":[]}),
            completed(json!([multipart])),
        ],
        vec![
            json!({"type":"response.output_text.delta", "sequence_number":1, "item_id":"answer",
                "output_index":0, "content_index":1, "delta":" answer", "logprobs":[]}),
            json!({"type":"response.output_text.delta", "sequence_number":2, "item_id":"answer",
                "output_index":0, "content_index":0, "delta":"A complete", "logprobs":[]}),
            completed(json!([multipart])),
        ],
    ];
    for remote in [false, true] {
        let expected = [
            "A complete answer",
            "A complete",
            "A complete answer",
            "Draft",
            "Draft",
            "A complete answer",
            " answer",
            " answer",
            " answerA complete",
        ];
        for (events, expected) in cases.clone().into_iter().zip(expected) {
            let server = server(events).await;
            let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            let turn = fixture.submit("Can you read this project?").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert!(page.runs.last().unwrap().error.is_none());
            assert_eq!(response(&page), expected);
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn preserves_tools_before_empty_completion() {
    for remote in [false, true] {
        let server = crate::agent_support::Server::empty_after_tool().await;
        let fixture =
            Fixture::packages(remote, ModelApi::Responses, &server.endpoint, &["files"]).await;
        std::fs::write(fixture.root.join("phase.txt"), "fixture").unwrap();
        let turn = fixture.submit("Read phase.txt and answer").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(page.runs.last().unwrap().error.is_none());
        assert_eq!(response(&page), crate::agent_support::phases::COMMENTARY);
        let parts = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn && entry.author != "user")
            .flat_map(|entry| &entry.parts)
            .collect::<Vec<_>>();
        let boundary = sailry_client::conversation::reply::boundary(
            page.entries
                .iter()
                .filter(|entry| entry.turn == turn && entry.author != "user")
                .flat_map(|entry| {
                    entry.parts.iter().enumerate().map(|(index, part)| {
                        sailry_client::conversation::reply::is_work(
                            part,
                            entry.commentary.contains_key(&index),
                        )
                    })
                }),
        );
        assert!(
            parts[boundary..]
                .iter()
                .all(|part| { !matches!(part, Part::Text(text) if !text.trim().is_empty()) })
        );
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolCall { .. }))
                .count(),
            1
        );
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolResult { .. }))
                .count(),
            1
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("phase.txt")).unwrap(),
            "fixture"
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn retains_incomplete_output() {
    for remote in [false, true] {
        let mut event = completed(json!([message("An unfinished answer")]));
        event["type"] = json!("response.incomplete");
        event["response"]["status"] = json!("incomplete");
        event["response"]["incomplete_details"] = json!({"reason":"max_output_tokens"});
        let server = server(vec![
            json!({"type":"response.output_text.delta", "sequence_number":1,
                "item_id":"answer", "output_index":0, "content_index":0,
                "delta":"An unfinished answer", "logprobs":[]}),
            event,
        ])
        .await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Answer this task").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Completed);
        assert!(run.error.is_none());
        assert_eq!(response(&page), "An unfinished answer");
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
