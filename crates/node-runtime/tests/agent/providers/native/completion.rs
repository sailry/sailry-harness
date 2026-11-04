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

async fn retains_streamed_and_final_content(fixture: &mut Fixture, _remote: bool, case_name: &str) {
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
        fixture
            .select(
                "retains_streamed_and_final_content",
                ModelApi::Responses,
                &server.endpoint,
            )
            .await;
        let turn = fixture.submit("Can you read this project?").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{case_name}: {}",
            format_args!("{:?}", page.runs)
        );
        assert!(page.runs.last().unwrap().error.is_none(), "{case_name}");
        assert_eq!(response(&page), expected, "{case_name}");
        assert_eq!(server.requests.lock().unwrap().len(), 1, "{case_name}");
    }
}

async fn retains_incomplete_output(fixture: &mut Fixture, _remote: bool, case_name: &str) {
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
    fixture
        .select(
            "retains_incomplete_output",
            ModelApi::Responses,
            &server.endpoint,
        )
        .await;
    let turn = fixture.submit("Answer this task").await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    let run = page.runs.last().unwrap();
    assert_eq!(run.status, Status::Completed, "{case_name}");
    assert!(run.error.is_none(), "{case_name}");
    assert_eq!(response(&page), "An unfinished answer", "{case_name}");
    assert_eq!(server.requests.lock().unwrap().len(), 1, "{case_name}");
}

#[tokio::test]
async fn preserves_tools_before_empty_completion() {
    for remote in [false, true] {
        let server = crate::agent_support::Server::empty_after_tool().await;
        let fixture =
            Fixture::packages(remote, ModelApi::Responses, &server.endpoint, &["files"]).await;
        std::fs::write(fixture.root.join("progress.txt"), "fixture").unwrap();
        let turn = fixture.submit("Read progress.txt and answer").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(page.runs.last().unwrap().error.is_none());
        assert_eq!(response(&page), crate::agent_support::progress::PROGRESS);
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
                .flat_map(|entry| &entry.parts)
                .map(sailry_client::conversation::reply::is_work),
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
            std::fs::read_to_string(fixture.root.join("progress.txt")).unwrap(),
            "fixture"
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

async fn reveals_empty_response(fixture: &mut Fixture, _remote: bool, case_name: &str) {
    let server = server(vec![completed(json!([]))]).await;
    fixture
        .select(
            "reveals_empty_response",
            ModelApi::Responses,
            &server.endpoint,
        )
        .await;
    let turn = fixture.submit("Answer this task").await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    let run = page.runs.iter().find(|run| run.turn == turn).unwrap();
    assert_eq!(run.status, Status::Completed, "{case_name}");
    assert!(run.error.is_none(), "{case_name}");
    assert_eq!(
        response(&page),
        goose_agent::inference::EMPTY_RESPONSE_MESSAGE,
        "{case_name}"
    );
    assert_eq!(server.requests.lock().unwrap().len(), 4, "{case_name}");
}

#[tokio::test]
async fn states() {
    use futures::FutureExt;
    for remote in [false, true] {
        let mut fixture = Fixture::empty(remote, ModelApi::Responses, "http://127.0.0.1:9").await;
        std::panic::AssertUnwindSafe(Box::pin(retains_streamed_and_final_content(
            &mut fixture,
            remote,
            "retains_streamed_and_final_content",
        )))
        .catch_unwind()
        .await
        .unwrap_or_else(|error| {
            eprintln!("step: retains_streamed_and_final_content, remote={remote}");
            std::panic::resume_unwind(error)
        });
        std::panic::AssertUnwindSafe(Box::pin(retains_incomplete_output(
            &mut fixture,
            remote,
            "retains_incomplete_output",
        )))
        .catch_unwind()
        .await
        .unwrap_or_else(|error| {
            eprintln!("step: retains_incomplete_output, remote={remote}");
            std::panic::resume_unwind(error)
        });
        std::panic::AssertUnwindSafe(Box::pin(reveals_empty_response(
            &mut fixture,
            remote,
            "reveals_empty_response",
        )))
        .catch_unwind()
        .await
        .unwrap_or_else(|error| {
            eprintln!("step: reveals_empty_response, remote={remote}");
            std::panic::resume_unwind(error)
        });
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
