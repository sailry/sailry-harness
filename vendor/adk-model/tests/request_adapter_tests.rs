#![cfg(feature = "openai")]

use adk_core::{Content, Llm, LlmRequest};
use adk_model::openai::{
    OpenAIReasoningEffort, OpenAIResponsesClient, OpenAIResponsesConfig, RequestAdapter,
};
use adk_model::retry::RetryConfig;
use adk_model::{OpenAICompatible, OpenAICompatibleConfig};
use futures::TryStreamExt;
use std::sync::Arc;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

#[tokio::test]
async fn request_customization_preserves_disabled_retries() {
    for responses in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("x-fixture", "custom"))
            .respond_with(ResponseTemplate::new(503).set_body_json(
                serde_json::json!({"error":{"message":"retry fixture","type":"server_error"}}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let adapter: RequestAdapter = Arc::new(|body, headers| {
            body["metadata"] = serde_json::json!({"fixture":"custom"});
            headers.insert("x-fixture", "custom".parse().unwrap());
            Ok(())
        });
        let model: Box<dyn Llm> = if responses {
            Box::new(
                OpenAIResponsesClient::new(
                    OpenAIResponsesConfig::new("fixture", "fixture").with_base_url(server.uri()),
                )
                .unwrap()
                .with_retry_config(RetryConfig::disabled())
                .with_request_adapter(adapter),
            )
        } else {
            Box::new(
                OpenAICompatible::new(
                    OpenAICompatibleConfig::new("fixture", "fixture").with_base_url(server.uri()),
                )
                .unwrap()
                .with_retry_config(RetryConfig::disabled())
                .with_request_adapter(adapter),
            )
        };
        let request = LlmRequest::new("fixture", vec![Content::new("user").with_text("Hello")]);
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut stream = model.generate_content(request, false).await?;
            stream.try_next().await
        })
        .await
        .expect("disabled retries must return the first HTTP failure");
        assert!(result.is_err());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["metadata"]["fixture"], "custom");
    }
}

#[tokio::test]
async fn default_headers_keep_max_reasoning_and_adapter_in_either_order() {
    for headers_first in [true, false] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(503).set_body_json(
                serde_json::json!({"error":{"message":"retry fixture","type":"server_error"}}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let adapter: RequestAdapter = Arc::new(|body, _headers| {
            body["metadata"] = serde_json::json!({"fixture":"custom"});
            Ok(())
        });
        let mut headers = http::HeaderMap::new();
        headers.insert("x-session", http::HeaderValue::from_static("conversation-1"));
        let client = OpenAIResponsesClient::new_with_reasoning_effort(
            OpenAIResponsesConfig::new("fixture", "fixture").with_base_url(server.uri()),
            OpenAIReasoningEffort::Max,
        )
        .unwrap()
        .with_retry_config(RetryConfig::disabled());
        let client = if headers_first {
            client.with_default_headers(headers).unwrap().with_request_adapter(adapter)
        } else {
            client.with_request_adapter(adapter).with_default_headers(headers).unwrap()
        };
        let request = LlmRequest::new("fixture", vec![Content::new("user").with_text("Hello")]);
        let result =
            async { client.generate_content(request, false).await?.try_next().await }.await;
        assert!(result.is_err());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(
            (
                &body["reasoning"]["effort"],
                &body["metadata"],
                requests[0].headers.get("x-session").and_then(|value| value.to_str().ok()),
            ),
            (
                &serde_json::json!("max"),
                &serde_json::json!({"fixture":"custom"}),
                Some("conversation-1"),
            ),
            "headers_first: {headers_first}",
        );
    }
}
