use super::*;
use goose_provider_types::conversation::token_usage::Usage;
use rmcp::model::CallToolRequestParams;

fn call() -> rmcp::model::CallToolRequestParams {
    CallToolRequestParams::new("read")
        .with_arguments(serde_json::from_value(json!({"path":"file"})).unwrap())
}

#[test]
fn captures_signatures_and_extra_metadata() {
    let metadata = [
        ("thoughtSignature".into(), json!("signature")),
        ("extra".into(), json!("provider")),
    ]
    .into();
    let message =
        Message::assistant().with_tool_request_with_metadata("call", Ok(call()), Some(&metadata));
    let response = response::convert(
        message,
        ProviderUsage::new("model".into(), Usage::default()),
        "model",
        "google",
    )
    .unwrap();
    assert_eq!(
        response.usage_metadata.as_ref().unwrap().prompt_token_count,
        -1
    );
    assert_eq!(
        response.provider_metadata.as_ref().unwrap()["gdk_tool_metadata"]["call"]["provider"]["extra"],
        "provider"
    );
    let (_, _, messages, _) = request::convert(
        LlmRequest {
            contents: vec![response.content.unwrap()],
            ..Default::default()
        },
        &ModelConfig::new("model"),
    )
    .unwrap();
    let request = messages[0].content[0].as_tool_request().unwrap();
    assert_eq!(request.id, "call");
    assert_eq!(
        request.metadata.as_ref().unwrap()["thoughtSignature"],
        "signature"
    );
}

#[test]
fn preserves_redacted_thinking_order() {
    let message = Message::assistant()
        .with_redacted_thinking("first")
        .with_thinking("Consider", "signed")
        .with_tool_request("call", Ok(call()))
        .with_redacted_thinking("last");
    let response = response::convert(
        message,
        ProviderUsage::new("model".into(), Usage::default()),
        "model",
        "anthropic",
    )
    .unwrap();
    let parts = response.content.as_ref().unwrap().parts.clone();
    assert_eq!(parts.len(), 3);
    let Part::ServerToolResponse {
        server_tool_response,
    } = &parts[2]
    else {
        panic!("native temporary boundary expected");
    };
    assert_eq!(server_tool_response["content"][0]["data"], "first");
    assert_eq!(server_tool_response["content"][2]["id"], "call");
    let (_, _, messages, _) = request::convert(
        LlmRequest {
            contents: vec![response.content.unwrap()],
            ..Default::default()
        },
        &ModelConfig::new("model"),
    )
    .unwrap();
    assert_eq!(messages[0].content.len(), 4);
    assert!(
        matches!(&messages[0].content[0], MessageContentBlock::RedactedThinking(thinking) if thinking.data=="first")
    );
    assert!(
        matches!(&messages[0].content[3], MessageContentBlock::RedactedThinking(thinking) if thinking.data=="last")
    );
}
