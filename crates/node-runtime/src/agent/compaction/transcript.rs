//! A summary-only copy; canonical replay and retained reasoning are untouched.
//! Follows Pi's summary serialization policy at 58c0b02ce552adab76014e7ed712ab7981359aa8
//! (packages/coding-agent/src/core/compaction/utils.ts): omit thinking and cap tool results.
use adk_core::{EmbeddedResource, Event, Part};
use serde_json::{Value, json};

const TOOL_RESULT_CHARS: usize = 2_000;

pub(super) fn encode(events: &[Event]) -> adk_core::Result<String> {
    let contents: Vec<_> = events
        .iter()
        .filter_map(|event| {
            let content = event
                .actions
                .compaction
                .as_ref()
                .map(|summary| &summary.compacted_content)
                .or_else(|| event.content())?;
            let parts: Vec<_> = content
                .parts
                .iter()
                .enumerate()
                .filter_map(|(index, item)| part(item).map(|value| (index, value)))
                .collect();
            if parts.is_empty() {
                return None;
            }
            let original = event
                .provider_metadata
                .get(crate::store::agent::INPUT_PART)
                .and_then(|index| index.parse::<usize>().ok())
                .filter(|index| matches!(content.parts.get(*index), Some(Part::Text { .. })))
                .and_then(|index| parts.iter().position(|(source, _)| *source == index));
            let mut value = json!({"author":event.author,
            "parts":parts.into_iter().map(|(_, value)| value).collect::<Vec<_>>()});
            if event.author == "user" {
                let source = event
                    .provider_metadata
                    .get(crate::store::agent::INPUT_SOURCE)
                    .filter(|source| {
                        matches!(source.as_str(), "user" | "parent" | "plugin" | "notice")
                    });
                value["input_source"] = json!(source);
                value["original_text_part"] = json!(original);
            }
            Some(value)
        })
        .collect();
    serde_json::to_string(&contents)
        .map_err(|_| super::invalid("context transcript could not be encoded"))
}

fn part(value: &Part) -> Option<Value> {
    Some(match value.provider_part() {
        Part::Thinking { .. } | Part::ProviderData { .. } => return None,
        Part::InlineData {
            mime_type,
            data,
            uri,
            ..
        } => json!({
            "mime_type":mime_type, "uri":uri, "bytes":data.len(), "contents_omitted":true
        }),
        Part::EmbeddedResource {
            resource: EmbeddedResource::Blob(blob),
        } => json!({
            "uri":blob.uri, "mime_type":blob.mime_type, "contents_omitted":true
        }),
        Part::EmbeddedResource {
            resource: EmbeddedResource::Text(text),
        } => json!({
            "uri":text.uri, "mime_type":text.mime_type, "contents_omitted":true
        }),
        Part::FunctionCall { id, name, args, .. } => json!({
            "function_call":{"id":id,"name":name,"args":args}
        }),
        Part::FunctionResponse {
            id,
            function_response,
            ..
        } => json!({
            "id":id, "function_response":{
                "name":function_response.name,
                "response":result(&function_response.response),
                "file_data":function_response.file_data,
                "inline_data":function_response.inline_data.iter().map(|data| json!({
                    "mime_type":data.mime_type, "bytes":data.data.len(), "contents_omitted":true
                })).collect::<Vec<_>>()
            }
        }),
        Part::ServerToolCall { server_tool_call } if server_tool_call["type"] == "reasoning" => {
            return None;
        }
        Part::ServerToolResponse {
            server_tool_response,
        } => json!({
            "server_tool_response":result(server_tool_response)
        }),
        other => serde_json::to_value(other).expect("ADK content is serializable"),
    })
}

fn result(value: &Value) -> Value {
    let text = value.to_string();
    let characters = text.chars().count();
    if characters <= TOOL_RESULT_CHARS {
        return value.clone();
    }
    let mut prefix = text.chars().take(TOOL_RESULT_CHARS).collect::<String>();
    prefix.push_str(&format!(
        "\n[... {} more characters truncated]",
        characters - TOOL_RESULT_CHARS
    ));
    json!({"text":prefix, "truncated":true, "error":value.get("error").is_some_and(|error| !error.is_null())
        || value.get("isError").and_then(Value::as_bool).unwrap_or(false)})
}

#[cfg(test)]
mod tests;
