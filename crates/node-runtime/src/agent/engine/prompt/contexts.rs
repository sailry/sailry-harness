//! Exact part identities stay with canonical context messages, outside model text.
use crate::agent::{
    canonical::PromptContext,
    model::observation::{Context, Source},
};
use crate::agent::{engine::conversation::messages_since_kickoff, model::Message};
use sailry_protocol::conversation::observation::CategoryKind;
use serde_json::json;
use std::collections::BTreeSet;

/// Session state repeats once per turn or when changed. Retained owner and
/// plugin parts repeat only after compaction hides their earlier copy.
pub(super) fn pending(conversation: &[Message], owned: &[&str], parts: Vec<String>) -> Vec<String> {
    let visible = |message: &&Message| message.is_turn_context() && message.is_agent_visible();
    let mut retained: BTreeSet<_> = conversation
        .iter()
        .filter(visible)
        .filter_map(identity)
        .flat_map(|context| context.retained)
        .collect();
    let last = messages_since_kickoff(conversation)
        .ok()
        .and_then(|turn| turn.iter().rev().find(visible))
        .and_then(identity);
    let parts: Vec<_> = parts
        .into_iter()
        .filter(|part| !owned.contains(&part.as_str()) || retained.insert(digest(part)))
        .collect();
    if parts.is_empty() {
        return parts;
    }
    let context = capture(&parts, owned);
    if last.is_none_or(|last| last.state != context.state) || !context.retained.is_empty() {
        parts
    } else {
        Vec::new()
    }
}

/// The canonical writer persists both the unchanged model text and its prompt
/// operation note, so identity survives session reload without another store.
pub(super) fn message(parts: &[String], owned: &[&str]) -> Option<Message> {
    let context = Context::new(
        parts
            .iter()
            .map(|text| Source {
                kind: CategoryKind::Injected,
                text: text.clone(),
            })
            .collect(),
    )?;
    let mut message = Message::user()
        .with_text(context.text())
        .with_visibility(false, true);
    message.metadata.turn_context = true;
    message
        .metadata
        .set_operation_note("prompt", "context", json!(capture(parts, owned)));
    Some(message)
}

fn identity(message: &Message) -> Option<PromptContext> {
    message
        .metadata
        .operation_note("prompt", "context")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn capture(parts: &[String], owned: &[&str]) -> PromptContext {
    let mut state = blake3::Hasher::new();
    let mut retained = Vec::new();
    for part in parts {
        if owned.contains(&part.as_str()) {
            retained.push(digest(part));
        } else {
            // Length prefixes distinguish one multiline part from several parts.
            state.update(&(part.len() as u64).to_le_bytes());
            state.update(part.as_bytes());
        }
    }
    PromptContext {
        state: state.finalize().to_hex().to_string(),
        retained,
    }
}

fn digest(part: &str) -> String {
    blake3::hash(part.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests;
