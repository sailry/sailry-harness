//! Derived from Goose 14741e9a, goose-context-management/src/format.rs.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use super::{Content, Message, Role};

pub(super) fn message(message: &Message) -> String {
    let parts: Vec<String> = message
        .content
        .iter()
        .filter_map(|content| match content {
            Content::Text { text } => Some(text.clone()),
            Content::Image { media_type, .. } => Some(format!("[image: {media_type}]")),
            Content::Document {
                name, media_type, ..
            } => Some(match name {
                Some(name) => format!("[document: {name} ({media_type})]"),
                None => format!("[document: {media_type}]"),
            }),
            Content::ToolCall(call) => Some(if call.error.is_none() {
                format!(
                    "tool_request({}): {}",
                    call.name,
                    serde_json::to_string(&call.arguments)
                        .unwrap_or_else(|_| "<<invalid json>>".to_owned())
                )
            } else {
                "tool_request: [error]".to_owned()
            }),
            Content::ToolResult(response) => Some(match &response.result {
                Ok(result) => {
                    let texts: Vec<_> = result
                        .content
                        .iter()
                        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
                        .collect();
                    if texts.is_empty() {
                        "tool_response: [non-text content]".to_owned()
                    } else {
                        format!("tool_response: {}", texts.join("\n"))
                    }
                }
                Err(_) => "tool_response: [error]".to_owned(),
            }),
            Content::Thinking { .. } | Content::RedactedThinking { .. } => None,
            Content::Error { message, .. } => Some(format!("error: {message}")),
        })
        .collect();
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    if parts.is_empty() {
        format!("[{role}]: <empty message>")
    } else {
        format!("[{role}]: {}", parts.join("\n"))
    }
}
