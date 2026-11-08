//! Turn boundaries derived from Goose 14741e9a operation.rs (Apache-2.0).
//! See third_party_licenses/goose-Apache-2.0.txt.
use crate::agent::{
    canonical::{EXECUTABLE_KEY, EXTERNAL_DISPATCH_KEY},
    model::{Content, ErrorKind, Message, Role, types::ToolCall},
};
use std::collections::BTreeSet;

pub(super) fn visible(mut message: Message) -> Option<Message> {
    if !message.is_agent_visible() {
        return None;
    }
    message.content.retain_mut(|content| match content {
        Content::Image { annotations, .. } => annotations
            .as_ref()
            .and_then(|annotations| annotations.get("audience"))
            .and_then(serde_json::Value::as_array)
            .is_none_or(|audience| audience.iter().any(|role| role == "assistant")),
        Content::ToolResult(response) => {
            if let Ok(result) = &mut response.result {
                result.content.retain(|content| {
                    use rmcp::model::ContentBlock;
                    let annotations = match content {
                        ContentBlock::Text(content) => content.annotations.as_ref(),
                        ContentBlock::Image(content) => content.annotations.as_ref(),
                        ContentBlock::Audio(content) => content.annotations.as_ref(),
                        ContentBlock::Resource(content) => content.annotations.as_ref(),
                        ContentBlock::ResourceLink(content) => content.annotations.as_ref(),
                        _ => None,
                    };
                    annotations
                        .and_then(|annotations| annotations.audience.as_ref())
                        .is_none_or(|audience| audience.contains(&rmcp::model::Role::Assistant))
                });
            }
            true
        }
        _ => true,
    });
    (!message.content.is_empty()).then_some(message)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EffectiveRole {
    User,
    Assistant,
    Tool,
}

pub(crate) fn effective_role(message: &Message) -> EffectiveRole {
    if message.role == Role::User
        && message
            .content
            .iter()
            .any(|content| matches!(content, Content::ToolResult(_)))
    {
        EffectiveRole::Tool
    } else if message.role == Role::User {
        EffectiveRole::User
    } else {
        EffectiveRole::Assistant
    }
}

pub(crate) fn messages_since_kickoff(messages: &[Message]) -> anyhow::Result<&[Message]> {
    let kickoff = |message: &Message| {
        message.role == Role::User && effective_role(message) != EffectiveRole::Tool
    };
    let start = messages
        .iter()
        .rposition(|message| kickoff(message) && message.is_user_visible())
        .or_else(|| {
            messages
                .iter()
                .rposition(|message| kickoff(message) && message.is_agent_visible())
        })
        .ok_or_else(|| anyhow::anyhow!("conversation has no kickoff message"))?;
    Ok(&messages[start..])
}

pub(crate) fn last_effective_role(messages: &[Message]) -> anyhow::Result<EffectiveRole> {
    messages
        .last()
        .map(effective_role)
        .ok_or_else(|| anyhow::anyhow!("conversation is empty"))
}

pub(crate) fn trailing_error(messages: &[Message]) -> Option<ErrorKind> {
    messages.last().and_then(Message::error_kind)
}

pub(crate) fn ends_turn(messages: &[Message]) -> bool {
    messages.last().is_some_and(|message| {
        message.role == Role::Assistant
            && message.error_kind().is_none()
            && !message
                .content
                .iter()
                .any(|content| matches!(content, Content::ToolCall(_)))
    })
}

pub(crate) fn assistant_turn_count(messages: &[Message]) -> u32 {
    let mut count = 0;
    let mut previous = false;
    for message in messages {
        let assistant = message.role == Role::Assistant;
        if assistant && !previous {
            count += 1;
        }
        previous = assistant;
    }
    count
}

pub(crate) fn request_executable(call: &ToolCall) -> Option<bool> {
    call.tool_meta.as_ref()?.get(EXECUTABLE_KEY)?.as_bool()
}

pub(crate) fn external(call: &ToolCall) -> bool {
    call.tool_meta
        .as_ref()
        .and_then(|meta| meta.get(EXTERNAL_DISPATCH_KEY))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

pub(crate) fn pending(messages: &[Message]) -> anyhow::Result<Vec<&ToolCall>> {
    let messages = messages_since_kickoff(messages)?;
    let answered: BTreeSet<_> = messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|content| match content {
            Content::ToolResult(result) => Some(result.id.as_str()),
            _ => None,
        })
        .collect();
    Ok(messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|content| match content {
            Content::ToolCall(call) => Some(call),
            _ => None,
        })
        .filter(|call| !external(call) && !answered.contains(call.id.as_str()))
        .collect())
}
