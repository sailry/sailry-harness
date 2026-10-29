//! Apply authorization policy to caller-owned protocol JSON, without an Agent engine.
use super::*;

pub(super) fn apply(prepared: &Prepared, body: &mut Value) -> Result<(), Fault> {
    let object = body
        .as_object_mut()
        .ok_or_else(|| Fault::invalid("model request must be an object"))?;
    for name in prepared.remove_params {
        object.remove(*name);
        if *name == "max_tokens" {
            object.remove("max_output_tokens");
        }
    }
    for (name, value) in &prepared.parameters {
        object.insert(name.clone(), value.clone());
    }
    let siwc =
        prepared.api == ModelApi::Responses && prepared.authentication == Authentication::ChatGpt;
    if siwc {
        // Public SIWC HTTP requirements differ from the GDK generation policy.
        // https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations
        for name in [
            "background",
            "conversation",
            "max_output_tokens",
            "max_tool_calls",
            "metadata",
            "moderation",
            "multi_agent",
            "prompt",
            "prompt_cache_retention",
            "safety_identifier",
            "temperature",
            "top_logprobs",
            "top_p",
            "truncation",
            "user",
            "previous_response_id",
        ] {
            object.remove(name);
        }
        object.insert("store".into(), json!(false));
        object.insert("stream".into(), json!(true));
        object.insert("instructions".into(), json!(prepared.system.text));
        let items = object
            .entry("input")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| Fault::invalid("model response input must be an array"))?;
        items.retain(|item| item["role"] != "system");
    } else if prepared.system.placement == Placement::Instructions {
        if let Some(items) = object.get_mut("input").and_then(Value::as_array_mut) {
            items.retain(|item| !matches!(item["role"].as_str(), Some("system" | "developer")));
        }
    } else if !prepared.system.text.is_empty() {
        match prepared.api {
            ModelApi::Responses => {
                let items = object
                    .entry("input")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .ok_or_else(|| Fault::invalid("model response input must be an array"))?;
                items.insert(0, json!({"type":"message", "role":"system", "content":[{"type":"input_text", "text":prepared.system.text}]}));
            }
            ModelApi::ChatCompletions => {
                let messages = object
                    .entry("messages")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .ok_or_else(|| Fault::invalid("model messages must be an array"))?;
                messages.insert(0, json!({"role":"system", "content":prepared.system.text}));
            }
            ModelApi::Anthropic => {
                object.insert("system".into(), json!(prepared.system.text));
            }
            ModelApi::Gemini => {
                object.insert(
                    "systemInstruction".into(),
                    json!({"parts":[{"text":prepared.system.text}]}),
                );
            }
            _ => {
                return Err(Fault::invalid(
                    "direct model request protocol is unsupported",
                ));
            }
        }
    }
    if !prepared.names.forward.is_empty() {
        if let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) {
            for tool in tools {
                // Vendor-owned server tools keep their declared protocol identity.
                let function = tool["type"] == "function" || tool.get("type").is_none();
                if !function {
                    continue;
                }
                if tool.get("function").is_some() {
                    rename(&mut tool["function"]["name"], &prepared.names.forward);
                } else {
                    rename(&mut tool["name"], &prepared.names.forward);
                }
            }
        }
        if let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) {
            for message in messages {
                if let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) {
                    for block in blocks {
                        if block["type"] == "tool_use" {
                            rename(&mut block["name"], &prepared.names.forward);
                        }
                    }
                }
                if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
                    for call in calls {
                        rename(&mut call["function"]["name"], &prepared.names.forward);
                    }
                }
            }
        }
        if let Some(items) = object.get_mut("input").and_then(Value::as_array_mut) {
            for item in items {
                if item["type"] == "function_call" {
                    rename(&mut item["name"], &prepared.names.forward);
                }
            }
        }
    }
    Ok(())
}

fn rename(value: &mut Value, names: &BTreeMap<String, String>) {
    if let Some(replacement) = value.as_str().and_then(|name| names.get(name)) {
        *value = json!(replacement);
    }
}
