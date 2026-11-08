//! Request repair adapted from Goose 14741e9a conversation.rs (Apache-2.0).
//! See third_party_licenses/goose-Apache-2.0.txt.
use super::super::conversation::{EffectiveRole, effective_role, messages_since_kickoff};
use crate::agent::model::{Content, Message, Role};
use std::collections::BTreeSet;

pub(super) fn visible(messages: &[Message]) -> anyhow::Result<Vec<Message>> {
    let turn = messages_since_kickoff(messages)?;
    let start = messages.len() - turn.len();
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
        .enumerate()
        .filter_map(|(index, message)| {
            let mut message = super::super::conversation::visible(message.clone())?;
            message.content.retain(|content| match content {
                Content::Error { .. } => false,
                Content::ToolCall(call) if index < start => answered.contains(call.id.as_str()),
                _ => true,
            });
            (!message.content.is_empty()).then_some(message)
        })
        .collect())
}

pub(super) fn applies(messages: &[Message]) -> bool {
    if messages
        .iter()
        .rev()
        .find(|message| message.is_agent_visible())
        .is_some_and(|message| message.role == Role::Assistant && message.content.is_empty())
    {
        return false;
    }
    let Ok(visible) = visible(messages) else {
        return false;
    };
    visible.last().is_some_and(|message| {
        !message.content.is_empty()
            && matches!(
                effective_role(message),
                EffectiveRole::User | EffectiveRole::Tool
            )
    })
}

fn signed(content: &Content) -> bool {
    match content {
        Content::Thinking { signature, .. } => {
            signature.as_ref().is_some_and(|value| !value.is_empty())
        }
        Content::RedactedThinking { .. } => true,
        _ => false,
    }
}

fn empty(message: &Message) -> bool {
    message
        .content
        .iter()
        .all(|content| matches!(content, Content::Text { text } if text.is_empty()))
}

fn merge(messages: Vec<Message>, across_context: bool) -> Vec<Message> {
    let mut merged: Vec<Message> = Vec::new();
    for message in messages {
        if let Some(previous) = merged.last_mut()
            && effective_role(previous) == effective_role(&message)
            && (across_context
                || (previous.metadata.user_visible == message.metadata.user_visible
                    && previous.metadata.turn_context == message.metadata.turn_context))
        {
            previous.content.extend(message.content);
        } else {
            merged.push(message);
        }
    }
    merged
}

pub(in crate::agent::engine) fn prepare(
    mut messages: Vec<Message>,
    provider: &str,
) -> Vec<Message> {
    for message in &mut messages {
        if message
            .metadata
            .inference
            .as_ref()
            .is_some_and(|inference| inference.provider != provider)
        {
            message.content.retain(|content| !signed(content));
        }
        if message.role == Role::Assistant {
            let mut content = Vec::new();
            for block in std::mem::take(&mut message.content) {
                match (content.last_mut(), block) {
                    (Some(Content::Text { text }), Content::Text { text: next }) => {
                        text.push_str(&next)
                    }
                    (_, block) => content.push(block),
                }
            }
            for block in &mut content {
                if let Content::Text { text } = block {
                    text.truncate(text.trim_end().len());
                }
            }
            message.content = content;
        }
        for content in &mut message.content {
            if let Content::ToolResult(response) = content
                && let Ok(result) = &mut response.result
                && !result.content.iter().any(|content| match content {
                    rmcp::model::ContentBlock::Text(text) => !text.text.is_empty(),
                    rmcp::model::ContentBlock::Image(_) => true,
                    rmcp::model::ContentBlock::Resource(resource) => match &resource.resource {
                        rmcp::model::ResourceContents::TextResourceContents { text, .. } => {
                            !text.is_empty()
                        }
                        rmcp::model::ResourceContents::BlobResourceContents { blob, .. } => {
                            !blob.is_empty()
                        }
                        _ => false,
                    },
                    _ => false,
                })
            {
                result
                    .content
                    .push(rmcp::model::ContentBlock::text("(empty result)"));
            }
        }
    }
    messages.retain(|message| !empty(message));
    let mut pending = BTreeSet::new();
    for message in &mut messages {
        message
            .content
            .retain(|content| match (&message.role, content) {
                (
                    Role::User,
                    Content::ToolCall(_)
                    | Content::Thinking { .. }
                    | Content::RedactedThinking { .. },
                ) => false,
                (Role::User, Content::ToolResult(result)) => pending.remove(&result.id),
                (Role::Assistant, Content::ToolResult(_)) => false,
                (Role::Assistant, Content::ToolCall(call)) => {
                    pending.insert(call.id.clone());
                    true
                }
                _ => true,
            });
    }
    for message in &mut messages {
        message.content.retain(
            |content| !matches!(content, Content::ToolCall(call) if pending.contains(&call.id)),
        );
    }
    messages.retain(|message| !empty(message));
    let mut messages = merge(messages, false);
    let mut seen = Vec::new();
    for message in &mut messages {
        message.content.retain(|content| {
            if !signed(content) {
                return true;
            }
            if seen.contains(content) {
                return false;
            }
            seen.push(content.clone());
            true
        });
    }
    if messages
        .first()
        .is_some_and(|message| message.role == Role::Assistant)
    {
        messages.remove(0);
    }
    if messages
        .last()
        .is_some_and(|message| message.role == Role::Assistant)
    {
        messages.pop();
    }
    if messages.is_empty() {
        messages.push(Message::user().with_text("Hello"));
    }
    merge(messages, true)
}
