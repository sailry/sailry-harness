use super::*;
use adk_core::ErrorCategory;
use futures::StreamExt;

const PRIVATE: &str = "private echoed provider credential";

fn created() -> Value {
    let mut event = completion(json!([]));
    event["type"] = "response.created".into();
    event["response"]["status"] = "in_progress".into();
    event
}

fn failure(code: &str, kind: &str) -> Value {
    match kind {
        "nested" => json!({"type":"error", "sequence_number":2,
            "error":{"type":"invalid_request_error", "code":code,
                "message":PRIVATE, "param":null}}),
        "flat" => json!({"type":"error", "sequence_number":2,
            "code":code, "message":PRIVATE, "param":null}),
        "failed" => {
            let mut event = completion(json!([]));
            event["type"] = "response.failed".into();
            event["response"]["status"] = "failed".into();
            event["response"]["error"] = json!({"code":code, "message":PRIVATE});
            event
        }
        _ => panic!("unknown error fixture"),
    }
}

async fn client(events: Vec<Value>, compatible: bool) -> (MockServer, OpenAIResponsesClient) {
    let server = MockServer::start().await;
    let mut body = String::new();
    for event in events {
        body.push_str(&format!("data: {event}\r\n\r\n"));
    }
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let client = OpenAIResponsesClient::new(
        OpenAIResponsesConfig::new("test-key", "test-model")
            .with_base_url(server.uri())
            .with_open_responses_mode(compatible),
    )
    .unwrap();
    (server, client)
}

fn request() -> LlmRequest {
    LlmRequest::new("test-model", vec![Content::new("user").with_text("test")])
}

#[tokio::test]
async fn preserves_shared_usage_causes() {
    for compatible in [false, true] {
        for (code, category) in [
            ("subscription_sharing_usage_limit_exceeded", ErrorCategory::RateLimited),
            ("subscription_sharing_usage_unavailable", ErrorCategory::Unavailable),
        ] {
            for kind in ["nested", "flat", "failed"] {
                let (server, client) = client(
                    vec![
                        created(),
                        failure(code, kind),
                        failure(code, "failed"),
                        completion(json!([message("msg", json!([text("Not completed")]))])),
                    ],
                    compatible,
                )
                .await;
                let mut stream = client.generate_content(request(), true).await.unwrap();
                assert!(stream.next().await.unwrap().unwrap().partial);
                let error = stream.next().await.unwrap().unwrap_err();
                assert_eq!(error.code, format!("model.openai_responses.{code}"));
                assert_eq!(error.category, category);
                assert!(!error.retry.should_retry);
                assert!(!adk_model::retry::is_retryable_model_error(&error));
                assert!(!error.to_string().contains(PRIVATE));
                assert!(stream.next().await.is_none());
                assert_eq!(server.received_requests().await.unwrap().len(), 1);
            }
        }
    }
}

#[tokio::test]
async fn sanitizes_unknown_causes() {
    for compatible in [false, true] {
        for kind in ["nested", "flat", "failed"] {
            let (server, client) = client(vec![failure(PRIVATE, kind)], compatible).await;
            let mut stream = client.generate_content(request(), true).await.unwrap();
            let error = stream.next().await.unwrap().unwrap_err();
            assert_eq!(error.code, "model.openai_responses.response_failed");
            assert!(!adk_model::retry::is_retryable_model_error(&error));
            assert!(!error.to_string().contains(PRIVATE));
            assert!(stream.next().await.is_none());
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }
}

#[tokio::test]
async fn stops_after_authoritative_completion() {
    for compatible in [false, true] {
        let results = responses(
            &[
                completion(json!([message("msg", json!([text("Completed")]))])),
                failure("subscription_sharing_usage_limit_exceeded", "nested"),
            ],
            compatible,
            "",
        )
        .await;
        assert_eq!(text_parts(&results), ["Completed"]);
    }
}

#[tokio::test]
async fn accepts_nullable_error_fields() {
    for compatible in [false, true] {
        let mut event = failure("subscription_sharing_usage_limit_exceeded", "nested");
        event["error"]["code"] = Value::Null;
        let result = try_responses(&[event], compatible, "").await;
        assert_eq!(result.unwrap_err().code, "model.openai_responses.response_failed");
    }
}

#[tokio::test]
async fn preserves_http_shared_usage_causes() {
    for compatible in [false, true] {
        for (code, status, category) in [
            ("subscription_sharing_usage_limit_exceeded", 429, ErrorCategory::RateLimited),
            ("subscription_sharing_usage_unavailable", 503, ErrorCategory::Unavailable),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/responses"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(
                        json!({"error":{"type":"invalid_request_error", "code":code,
                        "message":PRIVATE, "param":null}}),
                    ),
                )
                .expect(1)
                .mount(&server)
                .await;
            let client = OpenAIResponsesClient::new(
                OpenAIResponsesConfig::new("test-key", "test-model")
                    .with_base_url(server.uri())
                    .with_open_responses_mode(compatible),
            )
            .unwrap();
            let error = client.generate_content(request(), true).await.err().unwrap();
            assert_eq!(error.code, format!("model.openai_responses.{code}"));
            assert_eq!(error.category, category);
            assert_eq!(error.details.upstream_status_code, Some(status));
            assert!(!error.retry.should_retry);
            assert!(!adk_model::retry::is_retryable_model_error(&error));
            assert!(!error.to_string().contains(PRIVATE));
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }
}

#[tokio::test]
async fn preserves_unknown_service_retry() {
    for compatible in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(503).set_body_json(json!({
                "error":{"type":"server_error", "code":"unknown_service_error",
                    "message":"Temporary failure", "param":null}
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = OpenAIResponsesClient::new(
            OpenAIResponsesConfig::new("test-key", "test-model")
                .with_base_url(server.uri())
                .with_open_responses_mode(compatible),
        )
        .unwrap()
        .with_retry_config(adk_model::retry::RetryConfig::disabled());
        let error = client.generate_content(request(), true).await.err().unwrap();
        assert_eq!(error.code, "model.openai_responses.unavailable");
        assert_eq!(error.category, ErrorCategory::Unavailable);
        assert_eq!(error.details.upstream_status_code, Some(503));
        assert!(error.retry.should_retry);
        assert!(adk_model::retry::is_retryable_model_error(&error));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}
