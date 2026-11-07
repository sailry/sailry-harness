use super::*;

#[tokio::test]
async fn classifies_http_rejections() {
    for (status, body, expected) in [
        (
            400,
            json!({"error":{"message":"maximum context length exceeded",
                "code":"context_length_exceeded","type":"invalid_request_error"}}),
            ErrorKind::ContextOverflow,
        ),
        (
            401,
            json!({"error":{"message":"invalid authentication","type":"authentication_error"}}),
            ErrorKind::Authentication,
        ),
        (
            403,
            json!({"error":{"message":"permission denied"}}),
            ErrorKind::Authentication,
        ),
        (
            429,
            json!({"error":{"message":"rate limit exceeded","code":"rate_limit_exceeded"}}),
            ErrorKind::RateLimited,
        ),
        (
            503,
            json!({"error":{"message":"temporarily unavailable"}}),
            ErrorKind::Unavailable,
        ),
    ] {
        let headers = if status == 429 {
            "Retry-After: 7\r\n"
        } else {
            ""
        };
        let server = Server::start(vec![fixtures::rejection(status, body, headers)]).await;
        let provider = provider(ModelApi::Responses, server.endpoint.clone());
        let model = native(&provider, "fixture", "key", HeaderMap::new());
        let error = collect(&model, request("fixture"), CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.kind, expected, "{status}: {error:?}");
        assert_eq!(
            error.retry_after,
            (status == 429).then_some(Duration::from_secs(7))
        );
        assert_eq!(error.retryable(), matches!(status, 429 | 503));
        assert_eq!(server.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn classifies_stream_rejections() {
    for (code, message, kind) in [
        (
            "context_length_exceeded",
            "maximum context length exceeded",
            ErrorKind::ContextOverflow,
        ),
        (
            "rate_limit_exceeded",
            "rate limit exceeded",
            ErrorKind::RateLimited,
        ),
        (
            "server_error",
            "temporarily unavailable",
            ErrorKind::Unavailable,
        ),
        (
            "invalid_api_key",
            "invalid authentication",
            ErrorKind::Authentication,
        ),
    ] {
        let body = fixtures::sse([json!({
            "type":"response.failed","sequence_number":1,
            "response":{"id":"failed","object":"response","created_at":1,
                "status":"failed","model":"fixture","output":[],
                "error":{"code":code,"message":message}}
        })]);
        let server = Server::start(vec![body]).await;
        let provider = provider(ModelApi::Responses, server.endpoint.clone());
        let model = native(&provider, "fixture", "key", HeaderMap::new());
        let error = collect(&model, request("fixture"), CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.kind, kind, "{code}: {error:?}");
        assert_eq!(server.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn distinguishes_network_failures() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let provider = provider(ModelApi::Responses, endpoint);
    let model = native(&provider, "fixture", "key", HeaderMap::new());
    let error = collect(&model, request("fixture"), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Network);
    assert!(error.retryable());
    assert_eq!(error.retry_after, None);
}

#[tokio::test]
async fn redacts_captured_credentials() {
    let body = json!({"error":{
        "code":"invalid_api_key","message":"Rejected credential captured-key"
    }, "request":{"private":"private-request-sentinel"}});
    for reply in [
        fixtures::rejection(401, body.clone(), ""),
        fixtures::sse([json!({"type":"error","error":body["error"]})]),
    ] {
        let server = Server::start(vec![reply]).await;
        let provider = provider(ModelApi::Responses, server.endpoint.clone());
        let model = native(&provider, "fixture", "captured-key", HeaderMap::new());
        let error = collect(&model, request("fixture"), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(!error.message.contains("captured-key"));
        assert!(!error.message.contains("private-request-sentinel"));
        assert!(error.message.contains("[redacted]"));
        assert_eq!(server.finish().await.len(), 1);
    }
}

#[test]
fn retains_public_failure_categories() {
    use sailry_protocol::{ErrorCode, FaultDetails, ModelFailure};

    for (kind, reason) in [
        (ErrorKind::Authentication, ModelFailure::Unauthorized),
        (ErrorKind::RateLimited, ModelFailure::RateLimited),
        (ErrorKind::Quota, ModelFailure::RateLimited),
        (ErrorKind::ContextOverflow, ModelFailure::Request),
        (ErrorKind::Network, ModelFailure::Request),
    ] {
        let fault = super::super::errors::fault(Error::new(kind, "Captured rejection"));
        assert_eq!(fault.code, ErrorCode::Unavailable);
        assert_eq!(fault.message, "Captured rejection");
        assert_eq!(fault.details, Some(FaultDetails::ModelRequest { reason }));
    }
}
