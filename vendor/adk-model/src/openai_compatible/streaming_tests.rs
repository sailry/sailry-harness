use super::*;
use adk_core::FunctionResponseData;
use futures::TryStreamExt;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn client(server: &MockServer) -> OpenAICompatible {
    OpenAICompatible::new(
        OpenAICompatibleConfig::new("fixture-key", "fixture")
            .with_base_url(format!("{}/v1", server.uri())),
    )
    .unwrap()
    .with_retry_config(RetryConfig::disabled())
}

fn request(contents: Vec<Content>) -> LlmRequest {
    let mut request = LlmRequest::new("fixture", contents);
    request.tools.insert(
        "lookup".into(),
        json!({"description":"Read a fixture","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}),
    );
    request
}

fn usage() -> Value {
    json!({"prompt_tokens":8,"completion_tokens":4,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":2},"completion_tokens_details":{"reasoning_tokens":1}})
}

fn event(delta: Value, finish: Option<&str>, usage: Value) -> Value {
    json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":finish}],"usage":usage})
}

fn terminal(field: &str, finish: &str, separate_usage: bool) -> Vec<Value> {
    let mut delta = json!({field:"Inspect the file 🙂"});
    if finish == "tool_calls" {
        delta["tool_calls"] = json!([{"index":0,"id":"call-fixture","type":"function","function":{"name":"lookup","arguments":"{\"path\":\"fixture.txt\"}"}}]);
    } else {
        delta["content"] = json!("done");
    }
    let mut events = vec![event(
        delta,
        Some(finish),
        if separate_usage { Value::Null } else { usage() },
    )];
    if separate_usage {
        events.push(json!({"choices":[],"usage":usage()}));
    }
    events
}

async fn mount(server: &MockServer, events: Vec<Value>, count: u64) {
    let mut body = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    body.push_str("data: [DONE]\n\n");
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .expect(count)
        .mount(server)
        .await;
}

async fn collect(client: &OpenAICompatible, request: LlmRequest) -> Vec<LlmResponse> {
    client
        .generate_content(request, true)
        .await
        .unwrap()
        .try_collect::<Vec<_>>()
        .await
        .unwrap()
}

fn parts(response: &LlmResponse) -> &[Part] {
    response
        .content
        .as_ref()
        .map_or(&[], |content| content.parts.as_slice())
}

fn assert_usage(response: &LlmResponse) {
    let usage = response.usage_metadata.as_ref().unwrap();
    assert_eq!(usage.prompt_token_count, 8);
    assert_eq!(usage.candidates_token_count, 4);
    assert_eq!(usage.total_token_count, 12);
    assert_eq!(usage.cache_read_input_token_count, Some(2));
    assert_eq!(usage.thinking_token_count, Some(1));
}

#[tokio::test]
async fn retains_terminal_reasoning() {
    for field in ["reasoning_content", "reasoning"] {
        for finish in ["stop", "tool_calls"] {
            for separate_usage in [false, true] {
                let server = MockServer::start().await;
                mount(&server, terminal(field, finish, separate_usage), 1).await;
                let replies = collect(
                    &client(&server),
                    request(vec![Content::new("user").with_text("Read the fixture")]),
                )
                .await;
                assert_eq!(
                    replies.len(),
                    2,
                    "{field}, {finish}, separate_usage={separate_usage}"
                );
                let thinking = &replies[0];
                assert!(
                    matches!(parts(thinking), [Part::Thinking { thinking, signature:None }] if thinking == "Inspect the file 🙂")
                );
                assert!(thinking.partial);
                assert!(!thinking.turn_complete);
                assert!(thinking.finish_reason.is_none());
                assert!(thinking.usage_metadata.is_none());
                let completed = &replies[1];
                assert!(!completed.partial);
                assert!(matches!(completed.finish_reason, Some(FinishReason::Stop)));
                assert_eq!(completed.turn_complete, finish == "stop");
                assert_usage(completed);
                if finish == "tool_calls" {
                    assert!(
                        matches!(parts(completed), [Part::FunctionCall { name, args, id:Some(id), thought_signature:None }] if name == "lookup" && args == &json!({"path":"fixture.txt"}) && id == "call-fixture")
                    );
                } else {
                    assert!(matches!(parts(completed), [Part::Text { text }] if text == "done"));
                }
                assert_eq!(server.received_requests().await.unwrap().len(), 1);
            }
        }
    }
}

#[tokio::test]
async fn preserves_nonterminal_deltas() {
    for field in [None, Some("reasoning_content"), Some("reasoning")] {
        let server = MockServer::start().await;
        let mut first = json!({"content":"Hello "});
        let mut second = json!({"content":"world"});
        if let Some(field) = field {
            first[field] = json!("First ");
            second[field] = json!("second");
            if field == "reasoning_content" {
                first["reasoning"] = json!("Ignored fallback");
                second["reasoning"] = json!("Ignored fallback");
            }
        }
        mount(
            &server,
            vec![
                event(first, None, Value::Null),
                event(second, None, Value::Null),
                event(json!({}), Some("stop"), usage()),
            ],
            1,
        )
        .await;
        let replies = collect(
            &client(&server),
            request(vec![Content::new("user").with_text("Greet the fixture")]),
        )
        .await;
        let expected = if field.is_some() { 5 } else { 3 };
        assert_eq!(replies.len(), expected);
        let mut text = String::new();
        let mut thinking = String::new();
        for reply in &replies[..replies.len() - 1] {
            assert!(reply.partial);
            assert!(!reply.turn_complete);
            assert!(reply.finish_reason.is_none());
            assert!(reply.usage_metadata.is_none());
            assert_eq!(parts(reply).len(), 1);
            match &parts(reply)[0] {
                Part::Text { text: delta } => text.push_str(delta),
                Part::Thinking {
                    thinking: delta,
                    signature: None,
                } => thinking.push_str(delta),
                part => panic!("unexpected partial part: {part:?}"),
            }
        }
        assert_eq!(text, "Hello world");
        assert_eq!(thinking, if field.is_some() { "First second" } else { "" });
        let completed = replies.last().unwrap();
        assert!(parts(completed).is_empty());
        assert!(!completed.partial);
        assert!(completed.turn_complete);
        assert!(matches!(completed.finish_reason, Some(FinishReason::Stop)));
        assert_usage(completed);
    }
}

#[tokio::test]
async fn replays_terminal_tool_reasoning() {
    for field in ["reasoning_content", "reasoning"] {
        for replay in [
            None,
            Some(ReasoningReplayField::ReasoningContent),
            Some(ReasoningReplayField::Reasoning),
        ] {
            let server = MockServer::start().await;
            mount(&server, terminal(field, "tool_calls", false), 2).await;
            let mut client = client(&server);
            if let Some(replay) = replay {
                client = client.with_reasoning_replay_field(replay);
            }
            let user = Content::new("user").with_text("Read the fixture");
            let replies = collect(&client, request(vec![user.clone()])).await;
            let mut assistant = Content::new("model");
            for reply in &replies {
                assistant.parts.extend_from_slice(parts(reply));
            }
            assert_eq!(assistant.parts.len(), 2);
            let mut tool = Content::new("tool");
            tool.parts.push(Part::FunctionResponse {
                function_response: FunctionResponseData::new(
                    "lookup",
                    json!({"text":"Fixture contents"}),
                ),
                id: Some("call-fixture".into()),
                annotations: None,
            });
            collect(&client, request(vec![user, assistant, tool])).await;
            let requests = server.received_requests().await.unwrap();
            assert_eq!(requests.len(), 2);
            let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
            let assistant = &body["messages"][1];
            assert_eq!(assistant["role"], "assistant");
            assert_eq!(assistant["tool_calls"][0]["id"], "call-fixture");
            assert_eq!(assistant["tool_calls"][0]["function"]["name"], "lookup");
            let arguments: Value = serde_json::from_str(
                assistant["tool_calls"][0]["function"]["arguments"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(arguments, json!({"path":"fixture.txt"}));
            assert_eq!(body["messages"][2]["tool_call_id"], "call-fixture");
            for (name, selected) in [
                (
                    "reasoning_content",
                    Some(ReasoningReplayField::ReasoningContent),
                ),
                ("reasoning", Some(ReasoningReplayField::Reasoning)),
            ] {
                if replay.is_some() && replay == selected {
                    assert_eq!(assistant[name], "Inspect the file 🙂");
                } else {
                    assert!(assistant.get(name).is_none());
                }
            }
        }
    }
}
