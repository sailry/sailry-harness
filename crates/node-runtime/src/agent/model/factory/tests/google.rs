use super::*;

#[tokio::test]
async fn preserves_key_request_baseline() {
    let event = json!({
        "candidates":[{"content":{"role":"model","parts":[{"text":"Captured answer"}]},"finishReason":"STOP","index":0}],
        "usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":3,"totalTokenCount":15},
        "modelVersion":"gemini-fixture"
    });
    let server = Server::start(vec![format!("data: {event}\n\n")]).await;
    let registry = registry::Registry::default();
    let provider = provider(
        ModelApi::Gemini,
        format!("{}/proxy/v1beta", server.endpoint),
    );
    let native = native(
        &registry,
        &provider,
        "gemini-fixture",
        SessionId::new(),
        "google-key",
        HeaderMap::new(),
    )
    .unwrap();
    let config = ModelConfig::new("gemini-fixture").with_max_tokens(Some(64));
    let (answer, _) = collect_stream(
        native
            .stream(
                &config,
                "Captured system",
                &[Message::user().with_text("Read")],
                &[tool()],
            )
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(answer.as_concat_text(), "Captured answer");
    let requests = server.finish().await;
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.path,
        "/proxy/v1beta/models/gemini-fixture:streamGenerateContent?alt=sse"
    );
    assert_eq!(request.headers["x-goog-api-key"], "google-key");
    assert!(!request.headers.contains_key("authorization"));
    assert_eq!(request.headers["content-type"], "application/json");
    // The pre-refactor factory also delegates this exact body and key header to GoogleProvider.
    assert_eq!(
        request.body,
        json!({
            "systemInstruction":{"parts":[{"text":"Captured system"}]},
            "contents":[{"role":"user","parts":[{"text":"Read"}]}],
            "tools":{"functionDeclarations":[{
                "name":"read","description":"Read a file",
                "parametersJsonSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}
            }]},
            "generationConfig":{"maxOutputTokens":64}
        })
    );
}
