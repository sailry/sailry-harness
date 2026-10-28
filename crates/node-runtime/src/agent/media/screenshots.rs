//! Omit configured MCP images only in the transient model projection.
use goose_provider_types::conversation::message::{Message, MessageContentBlock};
use rmcp::model::ContentBlock;
use std::collections::BTreeMap;

const BATCH: usize = 5;

pub(in crate::agent) fn omit(messages: &mut [Message], servers: &[(Vec<String>, usize)]) {
    let calls: BTreeMap<_, _> = messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|content| match content {
            MessageContentBlock::ToolRequest(request) => request
                .tool_call
                .as_ref()
                .ok()
                .map(|call| (request.id.clone(), call.name.to_string())),
            _ => None,
        })
        .collect();
    for (aliases, keep) in servers {
        let mut results: Vec<_> = messages
            .iter_mut()
            .flat_map(|message| &mut message.content)
            .filter_map(|content| match content {
                MessageContentBlock::ToolResponse(response)
                    if calls
                        .get(&response.id)
                        .is_some_and(|name| aliases.contains(name)) =>
                {
                    response.tool_result.as_mut().ok()
                }
                _ => None,
            })
            .collect();
        let total: usize = results
            .iter()
            .map(|result| {
                result
                    .content
                    .iter()
                    .filter(|content| matches!(content, ContentBlock::Image(_)))
                    .count()
            })
            .sum();
        let mut remaining = total.saturating_sub(*keep) / BATCH * BATCH;
        for result in &mut results {
            if remaining == 0 {
                break;
            }
            let before = result.content.len();
            result.content.retain(|content| {
                if remaining > 0 && matches!(content, ContentBlock::Image(_)) {
                    remaining -= 1;
                    false
                } else {
                    true
                }
            });
            let omitted = before - result.content.len();
            if omitted == 0 {
                continue;
            }
            if let Some(value) = &mut result.structured_content
                && let Some(object) = value.as_object_mut()
            {
                let original = serde_json::to_string(object).ok();
                let previous = object
                    .get("omitted_screenshots")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                object.insert(
                    "omitted_screenshots".into(),
                    serde_json::json!(previous.saturating_add(omitted as u64)),
                );
                let updated = serde_json::to_string(object).ok();
                for content in &mut result.content {
                    if let ContentBlock::Text(text) = content
                        && original.as_deref() == Some(text.text.as_str())
                        && let Some(updated) = &updated
                    {
                        text.text.clone_from(updated);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, CallToolResult};
    use serde_json::json;

    fn response(name: &str, count: usize) -> Vec<Message> {
        let mut result = CallToolResult::structured(json!({"omitted_screenshots":2}));
        result
            .content
            .extend((0..count).map(|_| ContentBlock::image("AQ==", "image/png")));
        vec![
            Message::assistant()
                .with_tool_request(name, Ok(CallToolRequestParams::new(name.to_string()))),
            Message::user().with_tool_response(name, Ok(result)),
        ]
    }

    #[test]
    fn preserves_batch_boundaries() {
        for keep in [0, 3, 8] {
            for count in 0usize..20 {
                let mut messages = response("configured", count);
                messages.extend(response("other", 2));
                messages.push(Message::user().with_image("AQ==", "image/png"));
                let original = messages.clone();
                omit(&mut messages, &[(vec!["configured".into()], keep)]);
                let removed = count.saturating_sub(keep) / BATCH * BATCH;
                let MessageContentBlock::ToolResponse(response) = &messages[1].content[0] else {
                    unreachable!()
                };
                let result = response.tool_result.as_ref().unwrap();
                assert_eq!(
                    result
                        .content
                        .iter()
                        .filter(|part| matches!(part, ContentBlock::Image(_)))
                        .count(),
                    count - removed
                );
                assert_eq!(
                    result.structured_content.as_ref().unwrap()["omitted_screenshots"],
                    2 + removed
                );
                assert_eq!(&messages[2..], &original[2..]);
                if removed == 0 {
                    assert_eq!(messages, original);
                }
            }
        }
    }
}
