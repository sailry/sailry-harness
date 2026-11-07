use super::*;
use crate::agent::model::types::Inference;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, ResourceContents};

#[tokio::test]
async fn preserves_repeated_call_identities() {
    for api in [ModelApi::ChatCompletions, ModelApi::DeepSeek] {
        let server = Server::start(vec![fixtures::chat()]).await;
        let provider = provider(api, server.endpoint.clone());
        let model = native(&provider, "fixture", "key", HeaderMap::new());
        let mut input = request("fixture");
        for reason in ["First denial", "Second denial", "Completed"] {
            input.messages.extend([
                Message::user().with_text("Next task"),
                Message::assistant().with_tool_request(
                    "call-fixture-0",
                    Ok(CallToolRequestParams::new("read")
                        .with_arguments(json!({"path":reason}).as_object().unwrap().clone())),
                ),
                Message::user().with_tool_response(
                    "call-fixture-0",
                    Ok(CallToolResult::error(vec![ContentBlock::text(reason)])),
                ),
                Message::assistant().with_text("Finished"),
            ]);
        }
        let original = input.messages.clone();
        collect(&model, input.clone(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(input.messages, original);
        let captured = server.finish().await;
        let body = &captured[0].body;
        let messages = body["messages"].as_array().unwrap();
        let calls: Vec<_> = messages
            .iter()
            .filter_map(|message| message["tool_calls"].as_array())
            .flatten()
            .collect();
        let results: Vec<_> = messages
            .iter()
            .filter(|message| message["role"] == "tool")
            .collect();
        assert_eq!(calls.len(), 3, "{api:?}");
        assert_eq!(results.len(), 3, "{api:?}");
        for ((call, result), reason) in
            calls
                .iter()
                .zip(&results)
                .zip(["First denial", "Second denial", "Completed"])
        {
            assert_eq!(call["id"], "call-fixture-0", "{api:?}");
            assert_eq!(result["tool_call_id"], "call-fixture-0", "{api:?}");
            assert_eq!(result["content"], reason, "{api:?}");
            assert_eq!(
                serde_json::from_str::<Value>(call["function"]["arguments"].as_str().unwrap())
                    .unwrap(),
                json!({"path":reason}),
                "{api:?}"
            );
        }
        assert!(!body.to_string().contains("__sailry"), "{api:?}");
    }
}

#[tokio::test]
async fn preserves_tool_image_positions() {
    for (api, name, uploaded) in [
        (ModelApi::ChatCompletions, "gpt-4o", true),
        (ModelApi::ChatCompletions, "gpt-3.5-turbo", false),
        (ModelApi::DeepSeek, "deepseek-chat", false),
    ] {
        let server = Server::start(vec![fixtures::chat()]).await;
        let provider = provider(api, server.endpoint.clone());
        let model = native(&provider, name, "key", HeaderMap::new());
        let mut input = request(name);
        input.messages.extend([
            Message::assistant()
                .with_tool_request("images", Ok(CallToolRequestParams::new("read"))),
            Message::user().with_tool_response(
                "images",
                Ok(CallToolResult::success(vec![
                    ContentBlock::text("Before"),
                    ContentBlock::image("Zmlyc3Q=", "image/png"),
                    ContentBlock::text("After"),
                    ContentBlock::image("c2Vjb25k", "image/jpeg"),
                ])),
            ),
        ]);
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        let messages = captured[0].body["messages"].as_array().unwrap();
        let result = messages
            .iter()
            .find(|message| message["role"] == "tool")
            .unwrap();
        let note = if uploaded {
            "This tool result included an image that is uploaded in the next message."
        } else {
            "(tool image omitted: model does not support images)"
        };
        assert_eq!(
            result["content"],
            format!("Before {note} After {note}"),
            "{api:?} {name}"
        );
        let images: Vec<_> = messages
            .iter()
            .filter(|message| message["role"] == "user")
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .filter(|part| part["type"] == "image_url")
            .map(|part| part["image_url"]["url"].as_str().unwrap())
            .collect();
        if uploaded {
            assert_eq!(
                images,
                [
                    "data:image/png;base64,Zmlyc3Q=",
                    "data:image/jpeg;base64,c2Vjb25k",
                ],
                "{api:?} {name}"
            );
        } else {
            assert!(images.is_empty(), "{api:?} {name}");
        }
    }
}

#[tokio::test]
async fn replays_encrypted_reasoning_through_history() {
    let response = fixtures::sse([
        json!({
            "type":"response.created","sequence_number":0,
            "response":{"id":"response","object":"response","created_at":1,
                "model":"fixture","status":"in_progress","output":[]}
        }),
        json!({
            "type":"response.completed","sequence_number":1,
            "response":{"id":"response","object":"response","created_at":1,
                "model":"fixture","status":"completed","output":[
                    {"type":"reasoning","id":"rs_fixture","summary":[],
                        "encrypted_content":"encrypted-reasoning"},
                    {"type":"function_call","id":"fc_fixture","status":"completed",
                        "call_id":"stable-call","name":"read","arguments":"{}"}
                ],"usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15}}
        }),
    ]);
    let server = Server::start(vec![response, fixtures::responses(false)]).await;
    let provider = provider(ModelApi::Responses, server.endpoint.clone());
    let model = native(&provider, "fixture", "key", HeaderMap::new());
    let mut input = request("fixture");
    let events = collect(&model, input.clone(), CancellationToken::new())
        .await
        .unwrap();
    let signature = events
        .iter()
        .find_map(|event| match event {
            Event::ReasoningDelta { signature, .. } => signature.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(signature, "encrypted-reasoning");
    assert!(reasoning(&events).is_empty());
    let call = events
        .iter()
        .find_map(|event| match event {
            Event::ToolCall { call, .. } => Some(call.clone()),
            _ => None,
        })
        .unwrap();
    let mut message = Message::assistant()
        .with_thinking("", signature)
        .with_content(Content::ToolCall(call));
    message.id = Some("fixture-assistant".into());
    message.metadata.inference = Some(Inference {
        provider: format!("sailry:{}", provider.id),
        requested_model: "fixture".into(),
        ..Inference::default()
    });
    let database = turso::Builder::new_local(":memory:").build().await.unwrap();
    let db = database.connect().unwrap();
    let record = crate::agent::engine::history::capture(
        &db,
        sailry_protocol::TurnId::new(),
        &message,
        &Default::default(),
    )
    .await
    .unwrap();
    let restored = crate::agent::engine::history::project(&db, &record)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored, message);
    input.messages.extend([restored, result("stable-call")]);
    collect(&model, input, CancellationToken::new())
        .await
        .unwrap();
    let captured = server.finish().await;
    let replay = captured[1].body["input"].as_array().unwrap();
    let index = replay
        .iter()
        .position(|item| item["type"] == "reasoning")
        .unwrap();
    assert_eq!(replay[index]["summary"], json!([]));
    assert_eq!(replay[index]["encrypted_content"], "encrypted-reasoning");
    assert_eq!(replay[index + 1]["type"], "function_call");
    assert_eq!(replay[index + 1]["call_id"], "stable-call");
}

#[tokio::test]
async fn drops_thinking_from_a_previous_model() {
    for (requested, resolved, retained) in [
        ("claude-previous", None, false),
        ("claude-opus-4-8", None, true),
        ("deployment", Some("claude-opus-4-8"), true),
    ] {
        let server = Server::start(vec![fixtures::anthropic(false)]).await;
        let provider = provider(ModelApi::Anthropic, server.endpoint.clone());
        let model = native(&provider, "claude-opus-4-8", "key", HeaderMap::new());
        let mut input = request("claude-opus-4-8");
        let mut previous = Message::assistant()
            .with_thinking("Historical reasoning", "historical-signature")
            .with_content(Content::RedactedThinking {
                data: "historical-redacted".into(),
            })
            .with_text("Previous answer");
        previous.metadata.inference = Some(Inference {
            provider: format!("sailry:{}", provider.id),
            requested_model: requested.into(),
            resolved_model: resolved.map(str::to_owned),
            ..Inference::default()
        });
        input
            .messages
            .extend([previous, Message::user().with_text("Continue")]);
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        let content = captured[0].body["messages"].to_string();
        assert_eq!(content.contains("historical-signature"), retained);
        assert_eq!(content.contains("historical-redacted"), retained);
        assert_eq!(content.contains("Historical reasoning"), retained);
        assert!(content.contains("Previous answer"));
    }
}

fn blob(mime: &str, data: &str) -> ContentBlock {
    ContentBlock::resource(ResourceContents::BlobResourceContents {
        uri: "fixture://media".into(),
        mime_type: Some(mime.into()),
        blob: data.into(),
        meta: None,
    })
}

#[tokio::test]
async fn preserves_embedded_tool_media() {
    for (api, name, response) in [
        (
            ModelApi::Anthropic,
            "claude-opus-4-8",
            fixtures::anthropic(false),
        ),
        (
            ModelApi::Gemini,
            "gemini-3-flash-preview",
            fixtures::gemini(false),
        ),
        (ModelApi::Gemini, "gemini-2.5-pro", fixtures::gemini(false)),
    ] {
        let server = Server::start(vec![response]).await;
        let provider = provider(api, server.endpoint.clone());
        let model = native(&provider, name, "key", HeaderMap::new());
        let mut input = request(name);
        input.messages.push(
            Message::assistant()
                .with_tool_request("stable-call", Ok(CallToolRequestParams::new("read"))),
        );
        input.messages.push(Message::user().with_tool_response(
            "stable-call",
            Ok(CallToolResult::success(vec![
                ContentBlock::text("Before"),
                blob("image/png", "aW1hZ2U="),
                ContentBlock::image("ZGlyZWN0", "image/jpeg"),
                blob("application/pdf", "cGRm"),
                ContentBlock::text("After"),
            ])),
        ));
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        let body = &captured[0].body;
        assert!(!body.to_string().contains("__sailry_tool_media"));
        assert!(!body.to_string().contains("sailry-media:"));
        if api == ModelApi::Anthropic {
            let result = body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|message| message["content"].as_array().unwrap())
                .find(|block| block["type"] == "tool_result")
                .unwrap();
            let parts = result["content"].as_array().unwrap();
            assert_eq!(parts[0]["text"], "Before");
            assert_eq!(parts[1]["type"], "image");
            assert_eq!(parts[1]["source"]["data"], "aW1hZ2U=");
            assert_eq!(parts[2]["type"], "image");
            assert_eq!(parts[2]["source"]["data"], "ZGlyZWN0");
            assert_eq!(parts[3]["type"], "document");
            assert_eq!(parts[3]["source"]["media_type"], "application/pdf");
            assert_eq!(parts[3]["source"]["data"], "cGRm");
            assert_eq!(parts[4]["text"], "After");
        } else {
            let parts: Vec<_> = body["contents"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|content| content["parts"].as_array().unwrap())
                .collect();
            let result = parts
                .iter()
                .find_map(|part| part.get("functionResponse"))
                .unwrap();
            assert_eq!(result["name"], "read");
            assert!(result["response"].to_string().contains("Before"));
            assert!(result["response"].to_string().contains("After"));
            let media: Vec<_> = if name.starts_with("gemini-3") {
                result["parts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|part| &part["inlineData"])
                    .collect()
            } else {
                assert!(result.get("parts").is_none());
                parts
                    .iter()
                    .filter_map(|part| part.get("inline_data"))
                    .collect()
            };
            let mime = if name.starts_with("gemini-3") {
                "mimeType"
            } else {
                "mime_type"
            };
            assert_eq!(media.len(), 3);
            assert_eq!(media[0][mime], "image/png");
            assert_eq!(media[0]["data"], "aW1hZ2U=");
            assert_eq!(media[1][mime], "image/jpeg");
            assert_eq!(media[1]["data"], "ZGlyZWN0");
            assert_eq!(media[2][mime], "application/pdf");
            assert_eq!(media[2]["data"], "cGRm");
        }
    }
}
