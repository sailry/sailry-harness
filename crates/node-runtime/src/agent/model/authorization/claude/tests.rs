use super::*;

fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer synthetic-access"),
    );
    headers.insert("user-agent", HeaderValue::from_static("claude-cli/2.1.280"));
    headers.insert(
        "anthropic-beta",
        HeaderValue::from_static("claude-code-20250219,oauth-2025-04-20"),
    );
    headers
}

#[test]
fn preserves_instructions_and_headers() {
    let mut body = json!({"system":[{"type":"text","text":"Keep these instructions","cache_control":{"type":"ephemeral"}}],"messages":[],"thinking":{"type":"adaptive"}});
    let original = body["system"][0].clone();
    let mut request = HeaderMap::new();
    request.insert("x-api-key", HeaderValue::from_static("synthetic-access"));
    request.insert(
        "anthropic-beta",
        HeaderValue::from_static("structured-outputs-2025-11-13"),
    );
    request.insert("accept", HeaderValue::from_static("text/event-stream"));
    adapt(&mut body, &mut request, &headers()).unwrap();
    assert_eq!(body["system"][0]["text"], IDENTITY);
    assert_eq!(body["system"][1], original);
    assert!(!request.contains_key("x-api-key"));
    assert_eq!(request["authorization"], "Bearer synthetic-access");
    assert_eq!(request["accept"], "text/event-stream");
    let features = request["anthropic-beta"].to_str().unwrap();
    assert!(features.contains("structured-outputs-2025-11-13"));
    assert!(features.contains("oauth-2025-04-20"));
    assert!(!features.contains("interleaved-thinking"));
}

#[test]
fn maps_declared_and_historical_tools() {
    let mut body = json!({
        "system":"Original instructions",
        "tools":[{"name":"bash"},{"name":"read"},{"name":"sailry_custom"}],
        "tool_choice":{"type":"tool","name":"BASH"},
        "messages":[{"role":"assistant","content":[
            {"type":"tool_use","name":"read"},
            {"type":"tool_use","name":"sailry_custom"}
        ]}],
        "thinking":{"type":"enabled","budget_tokens":1024}
    });
    let mut request = HeaderMap::new();
    adapt(&mut body, &mut request, &headers()).unwrap();
    assert_eq!(body["system"][1]["text"], "Original instructions");
    assert_eq!(body["tools"][0]["name"], "Bash");
    assert_eq!(body["tools"][1]["name"], "Read");
    assert_eq!(body["tools"][2]["name"], "sailry_custom");
    assert_eq!(body["tool_choice"]["name"], "Bash");
    assert_eq!(body["messages"][0]["content"][0]["name"], "Read");
    assert_eq!(body["messages"][0]["content"][1]["name"], "sailry_custom");
    let features = request["anthropic-beta"].to_str().unwrap();
    assert!(features.contains("fine-grained-tool-streaming-2025-05-14"));
    assert!(features.contains("interleaved-thinking-2025-05-14"));
}

#[test]
fn rejects_invalid_parameters() {
    for mut body in [json!([]), json!({"system":42}), json!({"tools":[{}]})] {
        assert!(adapt(&mut body, &mut HeaderMap::new(), &headers()).is_err());
    }
}

#[tokio::test]
async fn restores_response_tool_names() {
    use adk_core::{Content, LlmResponse};
    use futures::TryStreamExt;
    let mut content = Content::new("model");
    for name in ["Bash", "sailry_custom"] {
        content.parts.push(Part::FunctionCall {
            name: name.into(),
            args: json!({"command":"isolated"}),
            id: Some(format!("call_{name}")),
            thought_signature: None,
        });
    }
    let response = LlmResponse {
        content: Some(content.clone()),
        partial: true,
        ..Default::default()
    };
    let stream = Box::pin(futures::stream::iter([Ok(response)]));
    let responses = restore(stream, vec!["bash".into(), "sailry_custom".into()])
        .try_collect::<Vec<_>>()
        .await
        .unwrap();
    content.parts[0] = Part::FunctionCall {
        name: "bash".into(),
        args: json!({"command":"isolated"}),
        id: Some("call_Bash".into()),
        thought_signature: None,
    };
    assert_eq!(
        serde_json::to_value(&responses[0].content).unwrap(),
        serde_json::to_value(Some(content)).unwrap()
    );
    assert!(responses[0].partial);
}
