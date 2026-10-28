//! Adapted from Goose ee8500d5, crates/goose/src/token_counter.rs (Apache-2.0).
//! The shared tokenizer is retained; an application token cache is unnecessary here.
use goose_provider_types::conversation::message::Message;
use std::sync::OnceLock;
static TOKENIZER: OnceLock<tiktoken_rs::CoreBPE> = OnceLock::new();
pub(crate) fn text(text: &str) -> usize {
    TOKENIZER
        .get_or_init(|| tiktoken_rs::o200k_base().expect("o200k tokenizer initializes"))
        .encode_with_special_tokens(text)
        .len()
}
pub(super) fn chat(system: &str, messages: &[Message]) -> usize {
    let mut tokens = if system.is_empty() {
        0
    } else {
        text(system) + 4
    };
    for message in messages.iter().filter(|message| message.is_agent_visible()) {
        tokens += 4;
        for content in &message.content {
            if let Some(value) = content.as_text() {
                tokens += text(value);
            } else if let Some(request) = content.as_tool_request() {
                if let Ok(call) = &request.tool_call {
                    tokens += text(&format!(
                        "{}:{}:{:?}",
                        request.id, call.name, call.arguments
                    ));
                }
            } else if let Some(value) = content.as_tool_response_text() {
                tokens += text(&value);
            }
        }
    }
    tokens + 3
}
pub(crate) fn records(records: &[crate::agent::canonical::Record]) -> u64 {
    use crate::agent::canonical::BlockKind;
    records
        .iter()
        .filter_map(|record| record.message())
        .map(|message| {
            let mut tokens = 4usize;
            for block in &message.content {
                tokens += match &block.kind {
                    BlockKind::Text { text: value } => text(value),
                    BlockKind::ToolRequest(call) => text(&format!(
                        "{}:{}:{:?}",
                        call.id.as_deref().unwrap_or_default(),
                        call.name,
                        call.arguments
                    )),
                    BlockKind::ToolResponse(result) => text(&result.result.value.to_string()),
                    _ => 0,
                };
            }
            tokens.saturating_add(3) as u64
        })
        .sum()
}
