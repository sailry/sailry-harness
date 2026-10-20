use super::*;
use async_openai::{error::OpenAIError, types::responses::ResponseStreamEvent};
use serde::de::Error as _;
use serde_json::{Value, json};

const SECRET: &str = "PRIVATE_SENTINEL_credential_prompt_text_delta_arguments";

fn mapped(error: serde_json::Error, body: &str, layer: Layer) -> AdkError {
    let mapped = super::super::map_error(OpenAIError::JSONDeserialize(error, body.into()), layer);
    for rendered in [
        mapped.to_string(),
        format!("{mapped:?}"),
        mapped.to_problem_json().to_string(),
        serde_json::to_string(&mapped.details.metadata).unwrap(),
    ] {
        assert!(!rendered.contains(SECRET), "content escaped error boundary");
    }
    assert!(std::error::Error::source(&mapped).is_none());
    assert_eq!(mapped.message, "OpenAI Responses API parse error");
    assert_eq!(mapped.code, "model.openai_responses.parse");
    assert!(!mapped.is_retryable());
    mapped
}

fn diagnostic(error: &AdkError) -> &Value {
    &error.details.metadata["openai_responses_parse"]
}

#[test]
fn identifies_missing_schema_field() {
    let body = json!({"type":"response.created","response":{},"delta":SECRET}).to_string();
    let error = serde_json::from_str::<ResponseStreamEvent>(&body).unwrap_err();
    let mapped = mapped(error, &body, Layer::StreamEvent);
    assert_eq!(
        diagnostic(&mapped),
        &json!({
            "layer":"stream_event", "category":"data", "line":0, "column":0,
            "cause":"missing_field", "field":"created_at", "event_type":"response.created"
        })
    );
}

#[test]
fn locates_missing_response_field() {
    let body = "{}";
    let error = serde_json::from_str::<async_openai::types::responses::Response>(body).unwrap_err();
    let mapped = mapped(error, body, Layer::ResponseBody);
    assert_eq!(
        diagnostic(&mapped),
        &json!({
            "layer":"response_body", "category":"data", "line":1, "column":2,
            "cause":"missing_field", "field":"created_at"
        })
    );
}

#[test]
fn distinguishes_layers() {
    for (layer, expected) in [
        (Layer::StreamSetup, "stream_setup"),
        (Layer::StreamEvent, "stream_event"),
        (Layer::ResponseBody, "response_body"),
    ] {
        let error = serde_json::from_str::<u64>("true").unwrap_err();
        let mapped = mapped(error, r#"{"type":"response.created"}"#, layer);
        assert_eq!(diagnostic(&mapped)["layer"], expected);
        assert_eq!(
            diagnostic(&mapped)["event_type"].as_str(),
            matches!(layer, Layer::StreamEvent).then_some("response.created")
        );
    }
}

#[test]
fn preserves_categories_and_locations() {
    let cases = [
        (serde_json::from_str::<Value>("{]").unwrap_err(), "syntax", 1, 2, "other"),
        (serde_json::from_str::<Value>("{\n").unwrap_err(), "eof", 2, 0, "other"),
        (serde_json::from_str::<u64>("true").unwrap_err(), "data", 1, 4, "invalid_type"),
        (serde_json::Error::io(std::io::Error::other(SECRET)), "io", 0, 0, "other"),
    ];
    for (error, category, line, column, cause) in cases {
        let mapped = mapped(error, SECRET, Layer::ResponseBody);
        assert_eq!(
            diagnostic(&mapped),
            &json!({
                "layer":"response_body", "category":category, "line":line,
                "column":column, "cause":cause
            })
        );
    }
}

#[test]
fn restricts_field_templates_and_names() {
    for (message, cause, field) in [
        ("missing field `sequence_number`".into(), "missing_field", Some("sequence_number")),
        ("duplicate field `id`".into(), "duplicate_field", Some("id")),
        (format!("missing field `{SECRET}`"), "missing_field", None),
        (format!("missing field `id` {SECRET}"), "missing_field", None),
        (format!("unknown variant `{SECRET}`, expected `completed`"), "unknown_variant", None),
        (format!("invalid value: {SECRET}"), "invalid_value", None),
        (SECRET.into(), "other", None),
    ] {
        let mapped = mapped(serde_json::Error::custom(message), SECRET, Layer::ResponseBody);
        assert_eq!(diagnostic(&mapped)["cause"], cause);
        assert_eq!(diagnostic(&mapped)["field"].as_str(), field);
    }
}

#[test]
fn rejects_arbitrary_event_names() {
    for name in [
        SECRET.to_string(),
        format!("response.{SECRET}"),
        "response..delta".into(),
        "response.tool-name.done".into(),
        format!("response.{}", "a".repeat(88)),
    ] {
        let body = json!({"type":name,"arguments":SECRET}).to_string();
        let error = serde_json::from_str::<ResponseStreamEvent>(&body).unwrap_err();
        let mapped = mapped(error, &body, Layer::StreamEvent);
        assert_eq!(diagnostic(&mapped)["event_type"], "unknown");
        assert_eq!(diagnostic(&mapped)["cause"], "unknown_variant");
    }
}

#[test]
fn preserves_technical_unknown_events() {
    for name in ["response.future", "response.grok_v2.reasoning_1.delta"] {
        let body = json!({"type":name,"arguments":SECRET}).to_string();
        let error = serde_json::from_str::<ResponseStreamEvent>(&body).unwrap_err();
        let mapped = mapped(error, &body, Layer::StreamEvent);
        assert_eq!(diagnostic(&mapped)["event_type"], name);
        assert_eq!(diagnostic(&mapped)["cause"], "unknown_variant");
    }
}

#[test]
fn omits_unavailable_event_types() {
    for body in [
        SECRET.into(),
        "{}".into(),
        json!({"type":42}).to_string(),
        json!({"type":"response.created","text":SECRET.repeat(2048)}).to_string(),
    ] {
        let mapped = mapped(serde_json::Error::custom(SECRET), &body, Layer::StreamEvent);
        assert!(diagnostic(&mapped).get("event_type").is_none());
    }
}

#[test]
fn omits_all_response_content() {
    for key in ["delta", "arguments", "text", "content", "message", "metadata", "request_id"] {
        let body = json!({"type":"response.output_text.delta",key:SECRET}).to_string();
        let error = serde_json::from_str::<ResponseStreamEvent>(&body).unwrap_err();
        let mapped = mapped(error, &body, Layer::StreamEvent);
        assert_eq!(diagnostic(&mapped)["event_type"], "response.output_text.delta");
    }
}
