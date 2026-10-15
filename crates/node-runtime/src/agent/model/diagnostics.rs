//! Copy only explicitly safe ADK diagnostics into the structured Node sink.
use adk_core::{AdkError, ErrorCategory, ErrorComponent};
use serde_json::{Map, Value};

const RESPONSES_PARSE: &str = "model.openai_responses.parse";
const PARSE_METADATA: &str = "openai_responses_parse";

pub(in crate::agent) fn cause(entry: &mut crate::logging::Entry, error: &AdkError) {
    entry.cause_code = Some(error.code);
    entry.cause_component = Some(component(error.component));
    entry.cause_category = Some(category(error.category));
    entry.upstream_status = error.details.upstream_status_code.map(u64::from);
    if error.code != RESPONSES_PARSE
        || error.component != ErrorComponent::Model
        || error.category != ErrorCategory::Internal
    {
        return;
    }
    let Some(Value::Object(value)) = error.details.metadata.get(PARSE_METADATA) else {
        return;
    };
    let Some(parsed) = parse(value) else {
        return;
    };
    entry.parse_layer = Some(parsed.layer);
    entry.parse_category = Some(parsed.category);
    entry.parse_cause = Some(parsed.cause);
    entry.parse_field = parsed.field;
    entry.parse_event = parsed.event;
    entry.parse_line = Some(parsed.line);
    entry.parse_column = Some(parsed.column);
}

struct Parse {
    layer: &'static str,
    category: &'static str,
    cause: &'static str,
    field: Option<&'static str>,
    event: Option<String>,
    line: u64,
    column: u64,
}

fn parse(value: &Map<String, Value>) -> Option<Parse> {
    if !value.keys().all(|key| {
        matches!(
            key.as_str(),
            "layer" | "category" | "line" | "column" | "cause" | "field" | "event_type"
        )
    }) {
        return None;
    }
    let layer = match value.get("layer")?.as_str()? {
        "stream_setup" => "stream_setup",
        "stream_event" => "stream_event",
        "response_body" => "response_body",
        _ => return None,
    };
    let category = match value.get("category")?.as_str()? {
        "io" => "io",
        "syntax" => "syntax",
        "data" => "data",
        "eof" => "eof",
        _ => return None,
    };
    let cause = match value.get("cause")?.as_str()? {
        "missing_field" => "missing_field",
        "duplicate_field" => "duplicate_field",
        "invalid_type" => "invalid_type",
        "invalid_value" => "invalid_value",
        "unknown_variant" => "unknown_variant",
        "other" => "other",
        _ => return None,
    };
    let field = match value.get("field") {
        None => None,
        Some(Value::String(value)) => Some(schema_field(value)?),
        Some(_) => return None,
    };
    let event = match value.get("event_type") {
        None => None,
        Some(Value::String(value)) if event_type(value) => Some(value.clone()),
        Some(_) => return None,
    };
    Some(Parse {
        layer,
        category,
        cause,
        field,
        event,
        line: value.get("line")?.as_u64()?,
        column: value.get("column")?.as_u64()?,
    })
}

fn event_type(value: &str) -> bool {
    matches!(value, "error" | "unknown")
        || (value.len() <= 96
            && value.strip_prefix("response.").is_some_and(|suffix| {
                suffix.split('.').all(|segment| {
                    !segment.is_empty()
                        && segment.bytes().all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                        })
                })
            }))
}

fn schema_field(value: &str) -> Option<&'static str> {
    const FIELDS: &[&str] = &[
        "type",
        "id",
        "object",
        "created_at",
        "model",
        "output",
        "status",
        "sequence_number",
        "response",
        "output_index",
        "item",
        "item_id",
        "content_index",
        "part",
        "delta",
        "text",
        "refusal",
        "arguments",
        "name",
        "summary_index",
        "partial_image_index",
        "annotation",
        "annotation_index",
        "input",
        "message",
        "code",
        "param",
        "role",
        "content",
        "annotations",
        "call_id",
        "summary",
        "encrypted_content",
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "cached_tokens",
        "reasoning_tokens",
        "input_tokens_details",
        "output_tokens_details",
        "url",
        "title",
        "start_index",
        "end_index",
        "file_id",
        "filename",
        "index",
        "token",
        "logprob",
        "bytes",
        "top_logprobs",
        "server_label",
        "tools",
        "result",
        "action",
        "queries",
        "query",
        "sources",
    ];
    FIELDS.iter().copied().find(|field| *field == value)
}

fn component(value: ErrorComponent) -> &'static str {
    use ErrorComponent::*;
    match value {
        Agent => "agent",
        Model => "model",
        Tool => "tool",
        Session => "session",
        Artifact => "artifact",
        Memory => "memory",
        Graph => "graph",
        Realtime => "realtime",
        Code => "code",
        Server => "server",
        Auth => "auth",
        Guardrail => "guardrail",
        Eval => "eval",
        Deploy => "deploy",
    }
}

fn category(value: ErrorCategory) -> &'static str {
    use ErrorCategory::*;
    match value {
        InvalidInput => "invalid_input",
        Unauthorized => "unauthorized",
        Forbidden => "forbidden",
        NotFound => "not_found",
        RateLimited => "rate_limited",
        Timeout => "timeout",
        Unavailable => "unavailable",
        Cancelled => "cancelled",
        Internal => "internal",
        Unsupported => "unsupported",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn error(value: Value) -> AdkError {
        let mut error = AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::Internal,
            RESPONSES_PARSE,
            "PRIVATE_MESSAGE",
        )
        .with_request_id("PRIVATE_REQUEST")
        .with_provider("PRIVATE_PROVIDER");
        error.details.metadata.insert(PARSE_METADATA.into(), value);
        error
            .details
            .metadata
            .insert("body".into(), json!("PRIVATE_BODY"));
        error
    }

    #[test]
    fn copies_valid_parse_diagnostics() {
        let error = error(json!({
            "layer":"stream_event",
            "category":"data",
            "line":7,
            "column":11,
            "cause":"missing_field",
            "field":"sequence_number",
            "event_type":"response.grok_v2.reasoning_1.delta"
        }));
        let mut entry =
            crate::logging::Entry::new(crate::logging::Area::Model, crate::logging::Phase::Failed);
        cause(&mut entry, &error);
        assert_eq!(entry.cause_code, Some(RESPONSES_PARSE));
        assert_eq!(entry.cause_component, Some("model"));
        assert_eq!(entry.cause_category, Some("internal"));
        assert_eq!(entry.parse_layer, Some("stream_event"));
        assert_eq!(entry.parse_category, Some("data"));
        assert_eq!(entry.parse_cause, Some("missing_field"));
        assert_eq!(entry.parse_field, Some("sequence_number"));
        assert_eq!(
            entry.parse_event.as_deref(),
            Some("response.grok_v2.reasoning_1.delta")
        );
        assert_eq!(entry.parse_line, Some(7));
        assert_eq!(entry.parse_column, Some(11));
    }

    #[test]
    fn rejects_untrusted_parse_metadata() {
        for value in [
            json!({"layer":"stream_event","category":"data","line":1,"column":2,"cause":"missing_field","field":"PRIVATE_FIELD"}),
            json!({"layer":"stream_event","category":"data","line":1,"column":2,"cause":"other","event_type":"PRIVATE_EVENT"}),
            json!({"layer":"stream_event","category":"data","line":1,"column":2,"cause":"other","body":"PRIVATE_BODY"}),
            json!({"layer":"stream_event","category":"data","line":-1,"column":2,"cause":"other"}),
        ] {
            let error = error(value);
            let mut entry = crate::logging::Entry::new(
                crate::logging::Area::Model,
                crate::logging::Phase::Failed,
            );
            cause(&mut entry, &error);
            assert!(entry.parse_layer.is_none());
            assert!(entry.parse_event.is_none());
            assert!(entry.parse_field.is_none());
        }
    }
}
