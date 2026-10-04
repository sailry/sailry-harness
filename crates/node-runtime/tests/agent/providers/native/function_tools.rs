use super::{ModelApi, Reply, Server, server};
use adk_core::{AdkError, Content, ErrorCategory, ErrorComponent, Llm, LlmRequest};
use adk_model::openai::{OpenAIResponsesClient, OpenAIResponsesConfig, RequestAdapter};
use adk_model::retry::RetryConfig;
use futures::TryStreamExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

fn parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "limit": {"type": "integer"}
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

async fn capture(declaration: Value, streaming: bool) -> Value {
    let server = Server::start(ModelApi::Responses, Reply::Text).await;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let records = captured.clone();
    let adapter: RequestAdapter = Arc::new(move |body, _| {
        records.lock().unwrap().push(body.clone());
        Err(AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::InvalidInput,
            "fixture.request_captured",
            "Request captured before HTTP",
        ))
    });
    let model = OpenAIResponsesClient::new(
        OpenAIResponsesConfig::new(server::KEY, "fixture-a").with_base_url(&server.endpoint),
    )
    .unwrap()
    .with_retry_config(RetryConfig::disabled())
    .with_request_adapter(adapter)
    .unwrap();
    let mut request = LlmRequest::new(
        "fixture-a",
        vec![Content::new("user").with_text("Inspect records")],
    );
    request.tools = HashMap::from([("search".into(), declaration.clone())]);
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let mut responses = model.generate_content(request.clone(), streaming).await?;
        responses.try_next().await
    })
    .await
    .expect("request capture should not wait for HTTP");
    let error = result.expect_err("the adapter must stop before HTTP");
    assert!(
        error
            .message
            .contains("Responses request customization failed"),
        "{error:?}"
    );
    assert_eq!(request.tools["search"], declaration);
    assert!(server.requests.lock().unwrap().is_empty());
    let mut captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 1);
    let body = captured.pop().unwrap();
    assert_eq!(body["stream"].as_bool().unwrap_or(false), streaming);
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    tools[0].clone()
}

#[tokio::test]
async fn ordinary_preserves_optional_parameters() {
    for streaming in [false, true] {
        let declaration = json!({
            "description": "Search records",
            "parameters": parameters()
        });
        let converted = capture(declaration.clone(), streaming).await;
        assert_eq!(
            converted,
            json!({
                "type": "function",
                "name": "search",
                "description": declaration["description"],
                "parameters": declaration["parameters"],
                "strict": false
            })
        );
    }
}

#[tokio::test]
async fn unknown_native_preserves_optional_parameters() {
    for streaming in [false, true] {
        let declaration = json!({
            "description": "Search records",
            "parameters": parameters(),
            "x-adk-openai-tool": {"type": "skill"}
        });
        let converted = capture(declaration.clone(), streaming).await;
        assert_eq!(
            converted,
            json!({
                "type": "function",
                "name": "search",
                "description": declaration["description"],
                "parameters": declaration["parameters"],
                "strict": false
            })
        );
    }
}

#[tokio::test]
async fn native_preserves_strictness() {
    for streaming in [false, true] {
        for strict in [Some(true), Some(false), None] {
            let mut descriptor = json!({
                "type": "function",
                "name": "native_search",
                "description": "Search records",
                "parameters": parameters()
            });
            if let Some(strict) = strict {
                descriptor["strict"] = json!(strict);
            }
            let converted = capture(json!({"x-adk-openai-tool": descriptor}), streaming).await;
            assert_eq!(converted, descriptor);
        }
    }
}
