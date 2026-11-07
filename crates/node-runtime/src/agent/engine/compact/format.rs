//! Derived from Goose 14741e9a, goose-context-management/src/format.rs.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use super::{Message, MessageContent};
use goose_provider_types::conversation::message::ActionRequiredData;

pub(super) fn message(message: &Message) -> String {
    let parts: Vec<String> = message
        .content
        .iter()
        .filter_map(|content| match content {
            MessageContent::Text(text) => Some(text.text.clone()),
            MessageContent::Image(image) => Some(format!("[image: {}]", image.mime_type)),
            MessageContent::Document(document) => Some(match &document.name {
                Some(name) => format!("[document: {name} ({})]", document.mime_type),
                None => format!("[document: {}]", document.mime_type),
            }),
            MessageContent::ToolRequest(request) => Some(match &request.tool_call {
                Ok(call) => format!(
                    "tool_request({}): {}",
                    call.name,
                    serde_json::to_string(&call.arguments)
                        .unwrap_or_else(|_| "<<invalid json>>".to_owned())
                ),
                Err(_) => "tool_request: [error]".to_owned(),
            }),
            MessageContent::ToolResponse(response) => Some(match &response.tool_result {
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
            MessageContent::ToolConfirmationRequest(request) => {
                Some(format!("tool_confirmation_request: {}", request.tool_name))
            }
            MessageContent::ActionRequired(action) => Some(match &action.data {
                ActionRequiredData::ToolConfirmation { tool_name, .. } => {
                    format!("action_required(tool_confirmation): {tool_name}")
                }
                ActionRequiredData::Elicitation { message, .. } => {
                    format!("action_required(elicitation): {message}")
                }
                ActionRequiredData::ElicitationResponse { id, .. } => {
                    format!("action_required(elicitation_response): {id}")
                }
                ActionRequiredData::ToolConfirmationResponse { id, .. } => {
                    format!("action_required(tool_confirmation_response): {id}")
                }
            }),
            MessageContent::Thinking(_) | MessageContent::RedactedThinking(_) => None,
            MessageContent::SystemNotification(notification) => {
                Some(format!("system_notification: {}", notification.msg))
            }
            MessageContent::Error(error) => Some(format!("error: {}", error.message)),
        })
        .collect();
    let role = match message.role {
        rmcp::model::Role::User => "user",
        rmcp::model::Role::Assistant => "assistant",
    };
    if parts.is_empty() {
        format!("[{role}]: <empty message>")
    } else {
        format!("[{role}]: {}", parts.join("\n"))
    }
}
