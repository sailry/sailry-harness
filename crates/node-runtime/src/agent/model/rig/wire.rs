use sailry_protocol::conversation::ModelApi;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) const HISTORY_TOOLS: &str = "__sailry_history_tools";
pub(super) const SYSTEM_ROLE: &str = "__sailry_system_role";
pub(super) const TOOL_IDS: &str = "__sailry_tool_ids";
pub(super) const SIGNATURE: &str = "__sailry_tool_signature";
pub(super) const CALL_ID: &str = "__sailry_call_id";

pub(super) fn prepare(body: &mut Value, api: ModelApi) {
    let role = body
        .as_object_mut()
        .and_then(|body| body.remove(SYSTEM_ROLE));
    if let Some(body) = body.as_object_mut()
        && body.remove(HISTORY_TOOLS) == Some(Value::Bool(true))
    {
        body.remove("tools");
        body.remove("tool_choice");
        body.remove("toolConfig");
    }
    match api {
        ModelApi::ChatCompletions | ModelApi::DeepSeek => chat(body, role),
        ModelApi::Gemini => gemini(body),
        _ => {}
    }
}

fn chat(body: &mut Value, role: Option<Value>) {
    let mut calls = BTreeMap::new();
    for message in body["messages"].as_array_mut().into_iter().flatten() {
        for call in message
            .get_mut("tool_calls")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if let Some(original) = call.as_object_mut().and_then(|call| call.remove(CALL_ID))
                && let Some(identity) = call["id"].as_str()
            {
                calls.insert(identity.to_owned(), original.clone());
                call["id"] = original;
            }
        }
        if message["role"] == "tool"
            && let Some(original) = message["tool_call_id"]
                .as_str()
                .and_then(|identity| calls.get(identity))
        {
            message["tool_call_id"] = original.clone();
        }
        if let Some(role) = &role
            && matches!(message["role"].as_str(), Some("system" | "developer"))
        {
            message["role"] = role.clone();
        }
        let Some(parts) = message["content"].as_array() else {
            continue;
        };
        let text: Option<Vec<_>> = parts
            .iter()
            .map(|part| {
                (part["type"] == "text")
                    .then(|| part["text"].as_str())
                    .flatten()
            })
            .collect();
        if let Some(text) = text {
            message["content"] = text.join("\n").into();
        }
    }
}

fn gemini(body: &mut Value) {
    let identities = body.as_object_mut().and_then(|body| body.remove(TOOL_IDS));
    let mut calls = identities
        .as_ref()
        .and_then(|identities| identities["calls"].as_array())
        .into_iter()
        .flatten();
    let mut results = identities
        .as_ref()
        .and_then(|identities| identities["results"].as_array())
        .into_iter()
        .flatten();
    if let Some(body) = body.as_object_mut() {
        for key in ["safetySettings", "toolConfig"] {
            if body.get(key).is_some_and(Value::is_null) {
                body.remove(key);
            }
        }
    }
    if let Some(tools) = body.get_mut("tools").and_then(Value::as_object_mut)
        && tools.get("codeExecution").is_some_and(Value::is_null)
    {
        tools.remove("codeExecution");
    }
    if let Some(system) = body.get_mut("systemInstruction") {
        parts(system);
    }
    for content in body["contents"].as_array_mut().into_iter().flatten() {
        parts(content);
        for part in content["parts"].as_array_mut().into_iter().flatten() {
            if let Some(part) = part.as_object_mut()
                && part.remove(SIGNATURE) == Some(Value::Bool(true))
            {
                part.entry("thoughtSignature")
                    .or_insert_with(|| "skip_thought_signature_validator".into());
            }
            if let Some(call) = part.get_mut("functionCall")
                && let Some(identity) = calls.next()
            {
                call["id"] = identity.clone();
            }
            if let Some(result) = part.get_mut("functionResponse") {
                if let Some(identity) = results.next() {
                    result["id"] = identity.clone();
                }
                let response = &result["response"];
                if response.get("content").is_some() {
                    continue;
                }
                let value = response.get("result").or_else(|| response.get("error"));
                let text = match value {
                    Some(Value::String(text)) => text.clone(),
                    Some(Value::Array(values)) => values
                        .iter()
                        .map(|value| {
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Some(value) => value.to_string(),
                    None => String::new(),
                };
                result["response"] = serde_json::json!({
                    "content":{"text":if text.is_empty() {"Tool call is done."} else {&text}}
                });
            }
        }
    }
}

fn parts(content: &mut Value) {
    for part in content["parts"].as_array_mut().into_iter().flatten() {
        if let Some(part) = part.as_object_mut()
            && part.get("thought") == Some(&Value::Bool(false))
        {
            part.remove("thought");
        }
    }
}
