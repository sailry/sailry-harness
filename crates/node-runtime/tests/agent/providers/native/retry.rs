use super::*;
use sailry_protocol::conversation::observation::Outcome;
use serde_json::{Value, json};

const MESSAGE: &str = "Provider request unavailable";

fn failure(code: &str) -> Value {
    json!({"type":"response.failed", "sequence_number":2,
        "response":{"id":"failed-response", "object":"response", "created_at":1,
            "model":"fixture-a", "status":"failed", "output":[],
            "error":{"code":code,
                "message":format!("{MESSAGE}; authorization {}", server::KEY)}}})
}

fn script(events: Vec<Value>, truncated: bool) -> server::Script {
    let mut body = events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    if !truncated {
        body.push_str("data: [DONE]\n\n");
    }
    server::Script {
        bytes: format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len() + if truncated { 100 } else { 0 },
        )
        .into_bytes(),
        ending: server::Ending::Close,
    }
}

#[tokio::test]
async fn recovers_nested_gateway_failure() {
    let server = Server::start(
        ModelApi::Responses,
        Reply::Scripted(Arc::new(vec![
            script(vec![failure("upstream_bad_gateway")], false),
            script(server::response_events(false, false, 1), false),
        ])),
    )
    .await;
    let fixture = Fixture::empty(ModelApi::Responses, &server.endpoint).await;
    let turn = fixture.submit("Recover the interrupted request").await;
    let page = finished_within(
        &fixture.client,
        fixture.session.id,
        turn,
        retry_fixture::budget(),
    )
    .await;
    let run = page.runs.last().unwrap();
    assert_eq!(run.status, Status::Completed);
    assert!(run.error.is_none());
    assert_eq!(completion::response(&page), server::ANSWER);
    assert!(!serde_json::to_string(&page).unwrap().contains(MESSAGE));
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body, requests[1].body);
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovers_truncated_transport() {
    for api in [
        ModelApi::Responses,
        ModelApi::ChatCompletions,
        ModelApi::Anthropic,
        ModelApi::Gemini,
    ] {
        let server = Server::start(api, Reply::Recover(script(Vec::new(), true))).await;
        let fixture = Fixture::empty(api, &server.endpoint).await;
        let turn = fixture.submit("Recover before any model output").await;
        let page = finished_within(
            &fixture.client,
            fixture.session.id,
            turn,
            retry_fixture::budget(),
        )
        .await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{api:?}: {:?}",
            page.runs
        );
        assert_eq!(completion::response(&page), server::ANSWER);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].body, requests[1].body);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn bounds_nested_gateway_retries() {
    let server = Server::start(
        ModelApi::Responses,
        Reply::Scripted(Arc::new(vec![script(
            vec![failure("upstream_bad_gateway")],
            false,
        )])),
    )
    .await;
    let fixture = Fixture::empty(ModelApi::Responses, &server.endpoint).await;
    let turn = fixture.submit("Keep the exhausted gateway failure").await;
    let page = finished_within(
        &fixture.client,
        fixture.session.id,
        turn,
        retry_fixture::budget(),
    )
    .await;
    let run = page.runs.last().unwrap();
    assert_eq!(run.status, Status::Failed);
    let error = run.error.as_ref().unwrap();
    assert!(error.message.contains("upstream_bad_gateway"));
    assert!(error.message.contains(MESSAGE));
    assert!(error.message.contains("[redacted]"));
    assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
    assert!(completion::response(&page).is_empty());
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 6);
    assert!(
        requests
            .iter()
            .all(|request| request.body == requests[0].body)
    );
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_terminal_failures() {
    for (code, reason) in [
        (
            "invalid_api_key",
            sailry_protocol::ModelFailure::Unauthorized,
        ),
        (
            "invalid_request_error",
            sailry_protocol::ModelFailure::Request,
        ),
    ] {
        let mut event = failure(code);
        event["response"]["error"]["message"] =
            json!(format!("{}{} after-secret", "x".repeat(490), server::KEY));
        let server = Server::start(
            ModelApi::Responses,
            Reply::Scripted(Arc::new(vec![script(vec![event], false)])),
        )
        .await;
        let fixture = Fixture::empty(ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Reject terminal provider failures").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Failed);
        let error = run.error.as_ref().unwrap();
        assert_eq!(
            error.details,
            Some(sailry_protocol::FaultDetails::ModelRequest { reason })
        );
        assert!(error.message.contains(code));
        assert!(error.message.contains("[redacted] after-secret"));
        let statistics = crate::observations::statistics(&fixture.client, fixture.session.id).await;
        for serialized in [
            serde_json::to_string(&page).unwrap(),
            serde_json::to_string(&statistics).unwrap(),
        ] {
            assert!(!serialized.contains(server::KEY));
            assert!(!serialized.contains(&server::KEY[..10]));
        }
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn retains_output_without_replay() {
    let delta = json!({"type":"response.output_text.delta", "sequence_number":1,
        "item_id":"partial", "output_index":0, "content_index":0,
        "delta":"Partial gateway answer", "logprobs":[]});
    let server = Server::start(
        ModelApi::Responses,
        Reply::Scripted(Arc::new(vec![
            script(vec![delta, failure("upstream_bad_gateway")], false),
            script(server::response_events(false, false, 1), false),
        ])),
    )
    .await;
    let fixture = Fixture::empty(ModelApi::Responses, &server.endpoint).await;
    let turn = fixture
        .submit("Preserve output from the failed stream")
        .await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    let run = page.runs.last().unwrap();
    assert_eq!(run.status, Status::Failed);
    assert!(
        run.error
            .as_ref()
            .unwrap()
            .message
            .contains("upstream_bad_gateway")
    );
    assert_eq!(completion::response(&page), "Partial gateway answer");
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn retains_usage_without_replay() {
    let event = json!({"id":"usage-before-failure", "object":"chat.completion.chunk",
        "created":1, "model":"fixture-a", "choices":[],
        "usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}});
    let server = Server::start(
        ModelApi::ChatCompletions,
        Reply::Scripted(Arc::new(vec![script(vec![event], true)])),
    )
    .await;
    let fixture = Fixture::empty(ModelApi::ChatCompletions, &server.endpoint).await;
    let turn = fixture.submit("Keep usage from the failed stream").await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Failed);
    assert!(completion::response(&page).is_empty());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    let statistics = crate::observations::statistics(&fixture.client, fixture.session.id).await;
    let request = statistics
        .observations
        .recent
        .iter()
        .find(|request| request.turn == turn)
        .unwrap();
    assert_eq!(request.attempts.len(), 1);
    assert_eq!(request.attempts[0].outcome, Outcome::Failed);
    assert_eq!(
        request.attempts[0].usage,
        Some(Usage {
            input: 12,
            output: 3,
            cached_input: 0,
            reasoning: 0,
        })
    );
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}
