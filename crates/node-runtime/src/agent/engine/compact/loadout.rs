//! Discovered tools that remain loaded after a compaction.
use goose_provider_types::conversation::{Conversation, message::MessageContent};
use std::collections::BTreeSet;

/// Loaded tools called or discovered in the agent-visible history the summary
/// replaces, in their first-seen order. Tools unused since the previous
/// compaction are unloaded; `tool_search` can find them again.
pub(super) fn retained(loaded: &[String], conversation: &Conversation) -> Vec<String> {
    let mut searches = BTreeSet::new();
    let mut used = BTreeSet::new();
    for content in conversation
        .messages()
        .iter()
        .filter(|message| message.is_agent_visible())
        .flat_map(|message| &message.content)
    {
        match content {
            MessageContent::ToolRequest(request) => {
                if let Ok(call) = &request.tool_call {
                    if call.name == "tool_search" {
                        searches.insert(request.id.as_str());
                    }
                    used.insert(call.name.to_string());
                }
            }
            MessageContent::ToolResponse(response) if searches.contains(response.id.as_str()) => {
                let found = response
                    .tool_result
                    .as_ref()
                    .ok()
                    .and_then(|result| result.structured_content.as_ref())
                    .and_then(|value| value["tools"].as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|tool| tool["name"].as_str());
                used.extend(found.map(str::to_owned));
            }
            _ => {}
        }
    }
    loaded
        .iter()
        .filter(|name| used.contains(name.as_str()))
        .cloned()
        .collect()
}
