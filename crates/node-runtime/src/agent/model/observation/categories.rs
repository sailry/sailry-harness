//! Estimate only content actually present in a provider-adapted request.
use sailry_protocol::conversation::observation::{Category, CategoryKind as Kind};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn estimate(body: &Value) -> Vec<Category> {
    let mut counts = BTreeMap::<Kind, (u64, u64)>::new();
    for field in [
        "instructions",
        "system",
        "systemInstruction",
        "system_instruction",
    ] {
        if let Some(value) = body.get(field) {
            add(&mut counts, Kind::System, value);
        }
    }
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            add(&mut counts, Kind::Tools, tool);
        }
    }
    for field in ["messages", "contents", "input"] {
        match body.get(field) {
            Some(Value::Array(messages)) => {
                for message in messages {
                    message_parts(&mut counts, message);
                }
            }
            Some(value @ Value::String(_)) => add(&mut counts, Kind::User, value),
            _ => {}
        }
    }
    [
        Kind::System,
        Kind::Tools,
        Kind::User,
        Kind::Skills,
        Kind::Injected,
        Kind::Assistant,
        Kind::Results,
    ]
    .into_iter()
    .map(|kind| {
        let known = !matches!(kind, Kind::Skills | Kind::Injected);
        let count = counts.get(&kind).copied().unwrap_or_default();
        Category {
            kind,
            items: known.then_some(count.0),
            tokens: known.then_some(count.1),
            method: known.then(|| "text_characters_quarter".into()),
        }
    })
    .collect()
}

fn message_parts(counts: &mut BTreeMap<Kind, (u64, u64)>, message: &Value) {
    let kind = match message.get("role").and_then(Value::as_str) {
        Some("system" | "developer") => Kind::System,
        Some("assistant" | "model") => Kind::Assistant,
        Some("tool" | "function") => Kind::Results,
        _ => match message.get("type").and_then(Value::as_str) {
            Some("function_call_output" | "computer_call_output") => Kind::Results,
            Some("function_call" | "reasoning" | "computer_call") => Kind::Assistant,
            _ => Kind::User,
        },
    };
    let parts = message.get("content").or_else(|| message.get("parts"));
    if let Some(Value::Array(parts)) = parts {
        for part in parts {
            let result = part.get("type").and_then(Value::as_str) == Some("tool_result")
                || part.get("functionResponse").is_some();
            add(counts, if result { Kind::Results } else { kind }, part);
        }
    } else {
        add(counts, kind, parts.unwrap_or(message));
    }
}

fn add(counts: &mut BTreeMap<Kind, (u64, u64)>, kind: Kind, value: &Value) {
    let entry = counts.entry(kind).or_default();
    entry.0 = entry.0.saturating_add(1);
    entry.1 = entry.1.saturating_add(characters(value).div_ceil(4));
}

fn characters(value: &Value) -> u64 {
    match value {
        Value::String(text) => {
            if text.starts_with("data:") {
                0
            } else {
                text.chars().count() as u64
            }
        }
        Value::Array(values) => values.iter().map(characters).sum(),
        Value::Object(values) => values
            .iter()
            // Binary payloads are not text token estimates. References and mime types remain.
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "data"
                        | "image_url"
                        | "file_data"
                        | "signature"
                        | "thoughtSignature"
                        | "encrypted_content"
                )
            })
            .map(|(_, value)| characters(value))
            .sum(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn separates_tool_results_and_omits_binary() {
        let categories = estimate(&json!({"messages":[{"role":"user","content":[
            {"type":"text","text":"hello"},
            {"type":"tool_result","content":"completed"},
            {"type":"image","source":{"data":"SECRET"}}
        ]}]}));
        assert_eq!(
            categories
                .iter()
                .find(|row| row.kind == Kind::Results)
                .unwrap()
                .items,
            Some(1)
        );
        assert_eq!(characters(&json!({"data":"private binary"})), 0);
        assert_eq!(
            categories
                .iter()
                .find(|row| row.kind == Kind::Skills)
                .unwrap()
                .tokens,
            None
        );
    }
}
