use super::*;
use futures::stream;
use serde_json::json;

#[tokio::test]
async fn parses_fragmented_completed_events() {
    let response = json!({"status":"completed", "output":[], "id":"Unicode fixture: 中文"});
    let content = format!(
        ": heartbeat\r\nevent: response.created\r\ndata: {{\"type\":\"response.created\"}}\r\n\r\nevent: response.completed\r\ndata: {{\"type\":\"response.completed\",\r\ndata: \"response\":{response}}}\r\n\r\n"
    );
    let chunks = content
        .as_bytes()
        .chunks(3)
        .map(|chunk| Ok::<_, ()>(chunk.to_vec()))
        .collect::<Vec<_>>();
    assert_eq!(events(stream::iter(chunks)).await.unwrap(), response);
}

#[tokio::test]
async fn rejects_incomplete_or_failed_streams() {
    for content in [
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: [DONE]\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{}}\n\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"private-access\"}}}\n\n",
        "data: {\"type\":\"error\",\"message\":\"private-access\"}\n\n",
    ] {
        let stream = stream::iter([Ok::<_, ()>(content.as_bytes())]);
        let error = events(stream).await.unwrap_err();
        assert!(
            !serde_json::to_string(&error.fault)
                .unwrap()
                .contains("private-access")
        );
    }
}

#[tokio::test]
async fn bounds_stream_body() {
    let bytes = vec![b'x'; super::super::super::network::MAX_BODY + 1];
    let error = events(stream::iter([Ok::<_, ()>(bytes)]))
        .await
        .unwrap_err();
    assert_eq!(error.fault.code, ErrorCode::Unavailable);
}

#[test]
fn sanitizes_server_tool_errors() {
    for (code, reason) in [
        ("too_many_requests", ModelFailure::RateLimited),
        ("unavailable", ModelFailure::Request),
        ("invalid_tool_input", ModelFailure::Request),
        ("max_uses_exceeded", ModelFailure::Request),
    ] {
        let error = upstream(
            &json!({"type":"web_search_tool_result_error", "error_code":code, "message":"private-access"}),
        );
        assert_eq!(
            error.fault.details,
            Some(FaultDetails::ModelRequest { reason })
        );
        assert!(!format!("{error:?}").contains("private-access"));
        assert_eq!(error.retry_at.is_some(), code == "too_many_requests");
    }
}

#[test]
fn distinguishes_tools_from_parameter_constraints() {
    for (message, unsupported) in [
        ("This model does not support web_search", true),
        ("web_search is not enabled for this plan", true),
        ("web_search_20250305 is not supported", true),
        ("Google Search tool is not supported for this model", true),
        ("Search Grounding is not supported for this model", true),
        ("Unsupported value: 'web_search'", true),
        ("Unknown name \"googleSearch\" at tools[0]", true),
        (
            "Unable to submit request because controlled generation is not supported with google_search tool",
            false,
        ),
        ("max_tokens is not supported while using web_search", false),
    ] {
        let value = json!({"error":{"code":400,"status":"INVALID_ARGUMENT","message":message}});
        let error = rejected(StatusCode::BAD_REQUEST, Some(&value), SystemTime::now());
        assert_eq!(
            error.fault.details == Some(FaultDetails::SearchUnsupported),
            unsupported,
            "{message}"
        );
        assert!(!error.fault.message.contains(message));
    }
}
