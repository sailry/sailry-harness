//! The Node offers review after a plain planning answer without another model call.
use super::*;
use crate::agent::model::{Content, Message};
use sailry_protocol::conversation::question::{Input, Spec};
pub(super) fn requested(message: &Message) -> bool {
    message.content.iter().any(|content| {
        matches!(content,
            Content::ToolCall(request) if request.error.is_none()
                && request.name == "ask_user"
                && serde_json::from_value::<Spec>(request.arguments.clone())
                    .is_ok_and(|spec| matches!(spec.input, Input::Plan)
                        && !spec.prompt.is_empty() && spec.validate().is_ok())
        )
    })
}
pub(super) fn completion(turn: TurnId, source: String) -> canonical::Record {
    crate::store::agent::review::event(turn, source)
}
