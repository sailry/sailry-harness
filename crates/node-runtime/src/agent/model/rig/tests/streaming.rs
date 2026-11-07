use super::*;

#[tokio::test]
async fn preserves_response_calls_and_usage() {
    let server = Server::start(vec![fixtures::responses(true), fixtures::responses(false)]).await;
    let provider = provider(ModelApi::Responses, format!("{}/v1", server.endpoint));
    let model = native(&provider, "fixture", "captured-key", HeaderMap::new());
    let mut input = request("fixture");
    let events = collect(&model, input.clone(), CancellationToken::new())
        .await
        .unwrap();
    let calls = events
        .iter()
        .filter_map(|event| match event {
            Event::ToolCall { call, .. } => Some(call),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1);
    let call = calls[0];
    assert_eq!(call.id, "stable-call");
    assert_eq!(call.name, "read");
    assert_eq!(call.arguments, json!({"path":"file"}));
    assert!(call.error.is_none());
    assert_eq!(events.last(), Some(&Event::Finish(Finish::ToolCalls)));
    let usage = events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Usage(usage) => Some(usage),
            _ => None,
        })
        .unwrap();
    assert_eq!(usage.input, Some(12));
    assert_eq!(usage.output, Some(3));
    assert_eq!(usage.total, Some(15));
    assert_eq!(usage.cache_read, Some(8));
    assert_eq!(usage.reasoning, Some(2));
    input.messages.push(Message {
        role: Role::Assistant,
        content: vec![Content::ToolCall(call.clone())],
    });
    input.messages.push(result(&call.id));
    let next = collect(&model, input, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(text(&next), fixtures::ANSWER);
    assert_eq!(next.last(), Some(&Event::Finish(Finish::Stop)));
    let captured = server.finish().await;
    assert_eq!(captured.len(), 2);
    let replay = captured[1].body["input"].as_array().unwrap();
    assert!(
        replay
            .iter()
            .any(|item| item["type"] == "function_call" && item["call_id"] == "stable-call")
    );
    assert!(replay.iter().any(|item| {
        item["type"] == "function_call_output"
            && item["call_id"] == "stable-call"
            && item["output"]
                .as_str()
                .is_some_and(|text| text.contains("File contents"))
    }));
}

#[tokio::test]
async fn exposes_chat_reasoning_and_text() {
    let server = Server::start(vec![fixtures::chat()]).await;
    let provider = provider(ModelApi::ChatCompletions, server.endpoint.clone());
    let model = native(&provider, "fixture", "key", HeaderMap::new());
    let events = collect(&model, request("fixture"), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(reasoning(&events), "Consider");
    assert_eq!(text(&events), fixtures::ANSWER);
    assert_eq!(events.last(), Some(&Event::Finish(Finish::Stop)));
    let usage = events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Usage(usage) => Some(usage),
            _ => None,
        })
        .unwrap();
    assert_eq!(usage.cache_read, Some(8));
    assert_eq!(usage.reasoning, Some(2));
    assert_eq!(server.finish().await.len(), 1);
}

#[tokio::test]
async fn preserves_anthropic_signed_thinking() {
    let server = Server::start(vec![fixtures::anthropic(true), fixtures::anthropic(false)]).await;
    let provider = provider(ModelApi::Anthropic, server.endpoint.clone());
    let model = native(&provider, "claude-opus-4-8", "key", HeaderMap::new());
    let mut input = request("claude-opus-4-8");
    let events = collect(&model, input.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(reasoning(&events), "Consider");
    let signature = events
        .iter()
        .filter_map(|event| match event {
            Event::ReasoningDelta { signature, .. } => signature.as_deref(),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(signature, "signed-thinking");
    assert!(events.iter().any(|event| matches!(
        event,
        Event::RedactedThinking { data, .. } if data == "redacted-data"
    )));
    let arguments = events
        .iter()
        .filter_map(|event| match event {
            Event::ToolCallDelta { arguments, .. } => Some(arguments.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(arguments, "{\"path\":\"file\"}");
    let call = events
        .iter()
        .find_map(|event| match event {
            Event::ToolCall { call, .. } => Some(call.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(call.id, "stable-call");
    assert_eq!(call.arguments, json!({"path":"file"}));
    let usage = events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Usage(usage) => Some(usage),
            _ => None,
        })
        .unwrap();
    assert_eq!(usage.cache_read, Some(8));
    assert_eq!(usage.cache_write, Some(4));
    input.messages.push(Message {
        role: Role::Assistant,
        content: vec![
            Content::Thinking {
                text: "Consider".into(),
                signature: Some(signature),
            },
            Content::RedactedThinking {
                data: "redacted-data".into(),
            },
            Content::ToolCall(call),
        ],
    });
    input.messages.push(result("stable-call"));
    collect(&model, input, CancellationToken::new())
        .await
        .unwrap();
    let captured = server.finish().await;
    let replay = captured[1].body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|message| message["content"].as_array().unwrap())
        .collect::<Vec<_>>();
    assert!(replay.iter().any(|block| block["type"] == "thinking"
        && block["thinking"] == "Consider"
        && block["signature"] == "signed-thinking"));
    assert!(
        replay
            .iter()
            .any(|block| block["type"] == "redacted_thinking" && block["data"] == "redacted-data")
    );
}

#[tokio::test]
async fn preserves_gemini_tool_signatures() {
    let server = Server::start(vec![fixtures::gemini(true), fixtures::gemini(false)]).await;
    let provider = provider(ModelApi::Gemini, format!("{}/v1beta", server.endpoint));
    let model = native(&provider, "gemini-3-flash-preview", "key", HeaderMap::new());
    let mut input = request("gemini-3-flash-preview");
    let events = collect(&model, input.clone(), CancellationToken::new())
        .await
        .unwrap();
    let call = events
        .iter()
        .find_map(|event| match event {
            Event::ToolCall { call, .. } => Some(call.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(call.id, "stable-call");
    assert_eq!(call.arguments, json!({"path":"file"}));
    assert_eq!(
        call.metadata.as_ref().unwrap()["thoughtSignature"],
        "c2lnbmVkLXRob3VnaHQ="
    );
    input.messages.push(Message {
        role: Role::Assistant,
        content: vec![Content::ToolCall(call)],
    });
    input.messages.push(result("stable-call"));
    collect(&model, input, CancellationToken::new())
        .await
        .unwrap();
    let captured = server.finish().await;
    assert_eq!(captured.len(), 2);
    assert!(
        captured[1].body["contents"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|message| message["parts"].as_array().unwrap())
            .any(|part| part["functionCall"]["id"] == "stable-call"
                && part["functionCall"]["name"] == "read"
                && part["thoughtSignature"] == "c2lnbmVkLXRob3VnaHQ=")
    );
}

#[tokio::test]
async fn rejects_unfinished_tool_calls() {
    use futures::StreamExt;

    let chunk = json!({
        "id":"chat","object":"chat.completion.chunk","created":1,"model":"fixture",
        "choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{
            "index":0,"id":"unfinished","type":"function",
            "function":{"name":"read","arguments":"{\"path\":"}
        }]},"finish_reason":null}]
    });
    let server = Server::start(vec![fixtures::sse([chunk])]).await;
    let provider = provider(ModelApi::ChatCompletions, server.endpoint.clone());
    let model = native(&provider, "fixture", "key", HeaderMap::new());
    let mut stream = model
        .call(request("fixture"), CancellationToken::new())
        .await
        .unwrap();
    let mut failure = None;
    while let Some(event) = stream.next().await {
        match event {
            Ok(Event::ToolCall { .. } | Event::Finish(_)) => {
                panic!("unfinished arguments became an executable call")
            }
            Err(error) => failure = Some(error),
            _ => {}
        }
    }
    assert_eq!(failure.unwrap().kind, ErrorKind::InvalidResponse);
    assert_eq!(server.finish().await.len(), 1);
}

#[tokio::test]
async fn keeps_unclosed_calls_nonexecutable() {
    use futures::StreamExt;
    use goose_provider_types::{
        base::collect_stream,
        conversation::message::{Message as LegacyMessage, MessageContentBlock},
        model::ModelConfig,
    };

    for (reason, arguments) in ["max_tokens", "end_turn"].into_iter().flat_map(|reason| {
        ["{\"path\":\"file\"}", "{\"path\":", ""]
            .into_iter()
            .map(move |arguments| (reason, arguments))
    }) {
        let body = fixtures::sse([
            json!({
                "type":"message_start",
                "message":{"id":"message","type":"message","role":"assistant",
                    "model":"claude-opus-4-8","content":[],"stop_reason":null,
                    "usage":{"input_tokens":12,"output_tokens":0}}
            }),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"tool_use","id":"unfinished","name":"read","input":{}}}),
            json!({"type":"content_block_delta","index":0,
                "delta":{"type":"input_json_delta","partial_json":arguments}}),
            json!({"type":"message_delta","delta":{"stop_reason":reason},
                "usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ]);
        let server = Server::start(vec![body.clone(), body]).await;
        let provider = provider(ModelApi::Anthropic, server.endpoint.clone());
        let legacy = crate::agent::model::factory::native(
            &registry::Registry::default(),
            &provider,
            "claude-opus-4-8",
            SessionId::new(),
            "key",
            HeaderMap::new(),
        )
        .unwrap();
        let (legacy, _) = collect_stream(
            legacy
                .stream(
                    &ModelConfig::new("claude-opus-4-8").with_max_tokens(Some(512)),
                    "Captured instructions",
                    &[LegacyMessage::user().with_text("Read the file")],
                    &[],
                )
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            legacy
                .content
                .iter()
                .filter(|content| matches!(content, MessageContentBlock::ToolRequest(call) if call.id == "unfinished" && call.tool_call.is_err()))
                .count(),
            1
        );
        let model = native(&provider, "claude-opus-4-8", "key", HeaderMap::new());
        let mut stream = model
            .call(request("claude-opus-4-8"), CancellationToken::new())
            .await
            .unwrap();
        let mut failure = None;
        let mut calls = 0;
        let mut finish = None;
        while let Some(event) = stream.next().await {
            match event {
                Ok(Event::ToolCall { call, .. }) => {
                    assert_eq!(call.id, "unfinished");
                    assert_eq!(call.error.as_ref().map(|error| error.code), Some(-32602));
                    assert!(call.metadata.as_ref().is_none_or(|metadata| {
                        !metadata.contains_key(super::super::anthropic::INCOMPLETE)
                    }));
                    calls += 1;
                }
                Ok(Event::Finish(reason)) => finish = Some(reason),
                Err(error) => failure = Some(error),
                _ => {}
            }
        }
        assert_eq!(calls, 1);
        assert_eq!(
            finish,
            Some(if reason == "max_tokens" {
                Finish::Length
            } else {
                Finish::ToolCalls
            })
        );
        assert!(failure.is_none());
        assert_eq!(server.finish().await.len(), 2);
    }
}
