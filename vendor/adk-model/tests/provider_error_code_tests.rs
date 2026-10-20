//! Provider API codes remain structured metadata without changing error semantics.
#![cfg(any(feature = "openai", feature = "deepseek", feature = "azure-ai"))]

use adk_core::{AdkError, Content, ErrorCategory, Llm, LlmRequest};
use adk_model::retry::RetryConfig;
use futures::StreamExt;
use std::time::Duration;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn first_error(model: &dyn Llm) -> AdkError {
    let request = LlmRequest::new("model", vec![Content::new("user").with_text("Hello")]);
    let mut stream = model.generate_content(request, false).await.expect("stream starts");
    stream.next().await.expect("stream yields an item").expect_err("request fails")
}

async fn check_cases(build: impl Fn(&str) -> Box<dyn Llm>, expected_diagnostic: &str) {
    let oversized = format!(r#"{{"error":{{"code":"{}"}}}}"#, "x".repeat(129));
    let cases = [
        (r#"{"error":{"code":"quota.exhausted"}}"#, Some("quota.exhausted"), 400),
        (r#"{"error":{"status":"RESOURCE_EXHAUSTED"}}"#, Some("RESOURCE_EXHAUSTED"), 400),
        (r#"{"error":{"code":"rate_limit"}}"#, Some("rate_limit"), 429),
        (r#"{"error":{"message":"missing code"}}"#, None, 400),
        (r#"{"error":{"code":"bad code"}}"#, None, 400),
        (r#"{"error":{"code":"fragment"}"#, None, 400),
        (oversized.as_str(), None, 400),
    ];
    for (body, expected_code, status) in cases {
        let server = MockServer::start().await;
        let response = ResponseTemplate::new(status).set_body_string(body);
        let response =
            if status == 429 { response.insert_header("retry-after", "7") } else { response };
        Mock::given(method("POST"))
            .and(path_regex("chat/completions$"))
            .respond_with(response)
            .mount(&server)
            .await;
        let model = build(&server.uri());
        let error = first_error(model.as_ref()).await;
        assert_eq!(
            error.category,
            if status == 429 { ErrorCategory::RateLimited } else { ErrorCategory::InvalidInput },
            "{body}"
        );
        assert_eq!(error.code, expected_diagnostic, "{body}");
        assert_eq!(error.details.upstream_status_code, Some(status), "{body}");
        assert_eq!(error.is_retryable(), status == 429, "{body}");
        assert_eq!(
            error.retry.retry_after(),
            (status == 429).then_some(Duration::from_secs(7)),
            "{body}"
        );
        assert!(error.message.contains(body), "{body}");
        assert_eq!(
            error.details.metadata.get("provider_error_code").and_then(serde_json::Value::as_str),
            expected_code,
            "{body}"
        );
    }
}

#[cfg(feature = "openai")]
#[tokio::test]
async fn compatible_preserves_only_valid_codes() {
    use adk_model::{OpenAICompatible, OpenAICompatibleConfig};

    check_cases(
        |uri| {
            Box::new(
                OpenAICompatible::new(
                    OpenAICompatibleConfig::new("key", "model").with_base_url(uri),
                )
                .unwrap()
                .with_retry_config(RetryConfig::disabled()),
            )
        },
        "model.openai_compat.api_error",
    )
    .await;
}

#[cfg(feature = "deepseek")]
#[tokio::test]
async fn deepseek_preserves_only_valid_codes() {
    use adk_model::{DeepSeekClient, DeepSeekConfig};

    check_cases(
        |uri| {
            Box::new(
                DeepSeekClient::new(DeepSeekConfig::new("key", "model").with_base_url(uri))
                    .unwrap()
                    .with_retry_config(RetryConfig::disabled()),
            )
        },
        "model.deepseek.api_error",
    )
    .await;
}

#[cfg(feature = "azure-ai")]
#[tokio::test]
async fn azure_ai_preserves_only_valid_codes() {
    use adk_model::{AzureAIClient, AzureAIConfig};

    check_cases(
        |uri| {
            Box::new(
                AzureAIClient::new(AzureAIConfig::new(uri, "key", "model"))
                    .unwrap()
                    .with_retry_config(RetryConfig::disabled()),
            )
        },
        "model.azure_ai.api_error",
    )
    .await;
}
