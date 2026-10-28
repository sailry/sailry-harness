//! The Node offers review after a plain planning answer without another model call.
use super::*;
use goose_provider_types::conversation::message::{Message, MessageContent};
use sailry_protocol::conversation::question::{Input, Spec};
pub(super) fn requested(message: &Message) -> bool {
    message.content.iter().any(|content|matches!(content,MessageContent::ToolRequest(request) if request.tool_call.as_ref().is_ok_and(|call|call.name=="ask_user" && serde_json::from_value::<Spec>(serde_json::Value::Object(call.arguments.clone().unwrap_or_default())).is_ok_and(|spec|matches!(spec.input,Input::Plan)&&!spec.prompt.is_empty()&&spec.validate().is_ok()))))
}
pub(super) fn completion(turn: TurnId, source: String) -> canonical::Record {
    crate::store::agent::review::event(turn, source)
}
