//! Discovered tools that remain loaded after a compaction.
use crate::agent::{
    model::{Content, Message},
    plugins::search,
};
use std::collections::BTreeSet;

/// Loaded tools called or discovered in the agent-visible history the summary
/// replaces, in their first-seen order. Tools unused since the previous
/// compaction are unloaded; `tool_search` can find them again.
pub(super) fn retained(loaded: &[String], conversation: &[Message]) -> Vec<String> {
    let mut searches = BTreeSet::new();
    let mut used = BTreeSet::new();
    for content in conversation
        .iter()
        .filter(|message| message.metadata.agent_visible)
        .flat_map(|message| &message.content)
    {
        match content {
            Content::ToolCall(call) => {
                if call.error.is_none() {
                    if call.name == search::NAME {
                        searches.insert(call.id.as_str());
                    }
                    used.insert(call.name.to_string());
                }
            }
            Content::ToolResult(response) if searches.contains(response.id.as_str()) => {
                if let Some(output) = response
                    .result
                    .as_ref()
                    .ok()
                    .and_then(|result| result.structured_content.as_ref())
                {
                    used.extend(search::discovered(output).map(str::to_owned));
                }
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
