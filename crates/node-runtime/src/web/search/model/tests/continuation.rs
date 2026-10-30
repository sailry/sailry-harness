use super::*;

#[tokio::test]
async fn preserves_paused_content_and_accumulates_usage() {
    let paused = json!({
        "type":"message", "role":"assistant", "stop_reason":"pause_turn",
        "content":[
            {"type":"server_tool_use","name":"web_search","id":"paused1","input":{"query":"search fixture"}},
            {"type":"web_search_tool_result","tool_use_id":"paused1","content":[
                {"type":"web_search_result","url":"https://example.com/paused","title":"Paused source","encrypted_content":"opaque-original"}
            ]}
        ],
        "usage":{"input_tokens":10,"output_tokens":3,"cache_read_input_tokens":2,"cache_creation_input_tokens":1}
    });
    let server =
        fixture::Server::sequence(vec![paused.clone(), response(ModelApi::Anthropic)]).await;
    let mut provider = auth::provider(Authentication::ClaudeOAuth, ModelApi::Anthropic);
    provider.endpoint = server.endpoint.clone();
    let prepared = prepare(
        &provider,
        &auth::credentials(Authentication::ClaudeOAuth),
        "fixture-model",
        auth::SESSION.parse().unwrap(),
        auth::REQUEST.parse().unwrap(),
        &input(),
        true,
    )
    .unwrap();
    let client = client();
    let parsed = execute(
        &client,
        prepared,
        &Run::new(&provider, "fixture-model", None, CancellationToken::new()),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(parsed.usage.unwrap()).unwrap(),
        json!({"input":113,"output":43,"cache_read":22,"cache_write":51,"reasoning":0})
    );
    assert_eq!(parsed.results.len(), 2);
    assert_eq!(parsed.results[0].url, "https://example.com/paused");
    assert_eq!(parsed.results[1].snippet, "Original source excerpt");
    let requests = server.captures().await;
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_headers(request, Authentication::ClaudeOAuth, ModelApi::Anthropic);
    }
    let mut expected = expected(ModelApi::Anthropic, Authentication::ClaudeOAuth);
    expected["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"assistant","content":paused["content"]}));
    assert_eq!(requests[1].body, expected);
    assert_eq!(
        requests[1].body["messages"][1]["content"][1]["content"][0]["encrypted_content"],
        "opaque-original"
    );
}

#[tokio::test]
async fn bounds_pauses_without_exposing_content() {
    let paused = json!({
        "type":"message", "role":"assistant", "stop_reason":"pause_turn",
        "content":[{"type":"text","text":"fixed-access fixed-refresh"}],
        "usage":{"input_tokens":1,"output_tokens":1}
    });
    let server = fixture::Server::sequence(vec![paused; 4]).await;
    let mut provider = auth::provider(Authentication::ClaudeOAuth, ModelApi::Anthropic);
    provider.endpoint = server.endpoint.clone();
    let prepared = prepare(
        &provider,
        &auth::credentials(Authentication::ClaudeOAuth),
        "fixture-model",
        auth::SESSION.parse().unwrap(),
        auth::REQUEST.parse().unwrap(),
        &input(),
        true,
    )
    .unwrap();
    let error = execute(
        &client(),
        prepared,
        &Run::new(&provider, "fixture-model", None, CancellationToken::new()),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.fault.message,
        "model search continuation limit reached"
    );
    let serialized = serde_json::to_string(&error.fault).unwrap();
    assert!(!serialized.contains("fixed-access"));
    assert!(!serialized.contains("fixed-refresh"));
    let requests = server.captures().await;
    assert_eq!(requests.len(), 4);
    for (index, request) in requests.iter().enumerate() {
        assert_eq!(
            request.body["messages"].as_array().unwrap().len(),
            index + 1
        );
    }
}
