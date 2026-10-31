use serde_json::{Value, json};

fn clear(block: &mut Value) {
    if let Some(object) = block.as_object_mut() {
        object.remove("cache_control");
    }
}

fn control() -> Value {
    json!({"type":"ephemeral","ttl":"1h"})
}

fn conversational(message: &Value) -> bool {
    message["role"] == "user"
        && match &message["content"] {
            Value::String(_) => true,
            Value::Array(blocks) => blocks
                .iter()
                .any(|block| matches!(block["type"].as_str(), Some("text" | "image"))),
            _ => false,
        }
}

fn decorate(message: &mut Value) -> bool {
    if let Some(text) = message["content"].as_str() {
        message["content"] = json!([{"type":"text","text":text,"cache_control":control()}]);
        return true;
    }
    if let Some(blocks) = message["content"].as_array_mut() {
        for block in blocks.iter_mut().rev() {
            if matches!(
                block["type"].as_str(),
                Some(
                    "thinking"
                        | "redacted_thinking"
                        | "fallback"
                        | "tool_addition"
                        | "tool_removal"
                )
            ) {
                continue;
            }
            if block.get("cache_control").is_some() {
                return false;
            }
            block["cache_control"] = control();
            return true;
        }
    }
    false
}

pub(super) fn apply(body: &mut Value) {
    let mut budget = 4usize;
    if let Some(tools) = body["tools"].as_array_mut() {
        for tool in tools.iter_mut() {
            clear(tool);
        }
        if let Some(tool) = tools
            .iter_mut()
            .rev()
            .find(|tool| tool["defer_loading"] != true)
        {
            tool["cache_control"] = control();
            budget -= 1;
        }
    }
    if let Some(blocks) = body["system"].as_array_mut() {
        for block in blocks.iter_mut() {
            clear(block);
        }
        let suffix = blocks
            .iter()
            .position(|block| {
                block["text"].as_str().is_some_and(|text| {
                    text.starts_with("<memories>") || text.starts_with("<project-context>")
                })
            })
            .unwrap_or(blocks.len());
        let anchor = if suffix == 0 {
            blocks.len().checked_sub(1)
        } else {
            Some(suffix - 1)
        };
        if let Some(anchor) = anchor {
            blocks[anchor]["cache_control"] = control();
            budget -= 1;
        }
    }
    let Some(messages) = body["messages"].as_array_mut() else {
        return;
    };
    for message in messages.iter_mut() {
        if let Some(blocks) = message["content"].as_array_mut() {
            for block in blocks {
                clear(block);
            }
        }
    }
    let mut trailing = Vec::new();
    let mut mark = None;
    let mut turns = 0usize;
    for (index, message) in messages.iter().enumerate().rev() {
        if message["clear_at"] == "next_user_message" {
            mark = Some(index);
            turns = 0;
            continue;
        }
        let tool_control = message["role"] == "system"
            && message["content"].as_array().is_some_and(|blocks| {
                !blocks.is_empty()
                    && blocks.iter().all(|block| {
                        matches!(
                            block["type"].as_str(),
                            Some("tool_addition" | "tool_removal")
                        )
                    })
            });
        if trailing.len() < 2 && !tool_control {
            trailing.push(index);
        }
        if conversational(message) {
            turns += 1;
        }
    }
    let mut candidates = Vec::new();
    if let Some(index) = trailing.first() {
        candidates.push(*index);
    }
    let stable_end = mark.unwrap_or(messages.len());
    if turns >= 15 {
        let mut remaining = turns % 15;
        for (index, message) in messages[..stable_end].iter().enumerate().rev() {
            if !conversational(message) {
                continue;
            }
            if remaining > 0 {
                remaining -= 1;
                continue;
            }
            remaining = 14;
            candidates.push(index);
            if candidates.len() > budget {
                break;
            }
        }
    }
    if let Some(index) = mark.and_then(|index| index.checked_sub(1)) {
        candidates.push(index);
    }
    candidates.extend(trailing);
    for index in candidates {
        if budget == 0 {
            break;
        }
        if decorate(&mut messages[index]) {
            budget -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_stable_head_and_recent_messages() {
        let mut body = json!({"tools":[{"name":"_read"},{"name":"_later","defer_loading":true}],"system":[{"text":"stable"},{"text":"<project-context>volatile"}],"messages":[{"role":"user","content":"question"},{"role":"assistant","content":[{"type":"thinking","thinking":"private"}]},{"role":"user","content":"next"}]});
        apply(&mut body);
        assert_eq!(body["tools"][0]["cache_control"], control());
        assert!(body["tools"][1].get("cache_control").is_none());
        assert_eq!(body["system"][0]["cache_control"], control());
        assert!(body["system"][1].get("cache_control").is_none());
        assert_eq!(
            body["messages"][2]["content"][0]["cache_control"],
            control()
        );
        assert!(
            body["messages"][1]["content"][0]
                .get("cache_control")
                .is_none()
        );
    }

    #[test]
    fn keeps_historical_checkpoint_with_four_breakpoints() {
        let messages: Vec<_> = (0..16)
            .map(|index| json!({"role":"user","content":format!("turn {index}")}))
            .collect();
        let mut body =
            json!({"tools":[{"name":"_read"}],"system":[{"text":"stable"}],"messages":messages});
        apply(&mut body);
        assert_eq!(
            body["messages"][14]["content"][0]["cache_control"],
            control()
        );
        assert_eq!(
            body["messages"][15]["content"][0]["cache_control"],
            control()
        );
        assert!(body["messages"][13]["content"].is_string());
    }
}
