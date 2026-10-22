//! Estimates follow adapted payload roles and exact Node-owned source fragments.
use super::Source;
use sailry_protocol::conversation::observation::{Category, CategoryKind as Kind};
use serde_json::Value;
use std::collections::BTreeMap;

type Counts = BTreeMap<Kind, (u64, u64)>;
pub(super) fn estimate(body: &Value, sources: &[Source]) -> Vec<Category> {
    let mut counts = Counts::new();
    for field in [
        "instructions",
        "system",
        "systemInstruction",
        "system_instruction",
    ] {
        if let Some(value) = body.get(field) {
            add(&mut counts, Kind::System, value, sources);
        }
    }
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            add(&mut counts, Kind::Tools, tool, &[]);
        }
    }
    for field in ["messages", "contents", "input"] {
        match body.get(field) {
            Some(Value::Array(messages)) => {
                for message in messages {
                    let kind = message_kind(message);
                    let content = message.get("content").or_else(|| message.get("parts"));
                    if let Some(Value::Array(parts)) = content {
                        for part in parts {
                            let result = part.get("type").and_then(Value::as_str)
                                == Some("tool_result")
                                || part.get("functionResponse").is_some();
                            add(
                                &mut counts,
                                if result { Kind::Results } else { kind },
                                part,
                                sources,
                            );
                        }
                    } else {
                        add(&mut counts, kind, content.unwrap_or(message), sources);
                    }
                }
            }
            Some(value @ Value::String(_)) => add(&mut counts, Kind::User, value, sources),
            _ => {}
        }
    }
    let recognized = [
        "messages",
        "contents",
        "input",
        "instructions",
        "system",
        "systemInstruction",
        "system_instruction",
    ]
    .iter()
    .any(|field| body.get(field).is_some());
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
        // Older content may lack provenance. Observed source fragments are a
        // lower bound, never a claim that historical unknown sources were zero.
        let identified = matches!(kind, Kind::Skills | Kind::Injected);
        let known = recognized && (!identified || counts.contains_key(&kind));
        let (items, chars) = counts.get(&kind).copied().unwrap_or_default();
        Category {
            kind,
            items: known.then_some(items),
            tokens: known.then_some(chars.div_ceil(4)),
            method: known.then(|| {
                if identified {
                    "identified_source_characters_quarter"
                } else {
                    "text_characters_quarter"
                }
                .into()
            }),
        }
    })
    .collect()
}
fn message_kind(message: &Value) -> Kind {
    match message.get("role").and_then(Value::as_str) {
        Some("system" | "developer") => Kind::System,
        Some("assistant" | "model") => Kind::Assistant,
        Some("tool" | "function") => Kind::Results,
        _ => match message.get("type").and_then(Value::as_str) {
            Some("function_call_output" | "computer_call_output") => Kind::Results,
            Some("function_call" | "reasoning" | "computer_call") => Kind::Assistant,
            _ => Kind::User,
        },
    }
}
fn add(counts: &mut Counts, kind: Kind, value: &Value, sources: &[Source]) {
    counts.entry(kind).or_default().0 += 1;
    measure(counts, kind, value, sources);
}
fn measure(counts: &mut Counts, kind: Kind, value: &Value, sources: &[Source]) {
    match value {
        Value::String(text) if !text.starts_with("data:") => {
            let mut ranges = Vec::new();
            for source in sources {
                if source.text.is_empty() {
                    continue;
                }
                for (start, matched) in text.match_indices(&source.text) {
                    ranges.push((start, start + matched.len(), source.kind));
                }
            }
            ranges.sort_by_key(|&(start, end, _)| (start, std::cmp::Reverse(end)));
            let mut until = 0;
            let mut attributed = 0;
            for (start, end, source) in ranges {
                if start < until {
                    continue;
                }
                until = end;
                let chars = text[start..end].chars().count() as u64;
                attributed += chars;
                let count = counts.entry(source).or_default();
                count.0 += 1;
                count.1 += chars;
            }
            counts.entry(kind).or_default().1 +=
                (text.chars().count() as u64).saturating_sub(attributed);
        }
        Value::Array(values) => {
            for value in values {
                measure(counts, kind, value, sources);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if !matches!(
                    key.as_str(),
                    "data"
                        | "image_url"
                        | "file_data"
                        | "signature"
                        | "thoughtSignature"
                        | "encrypted_content"
                ) {
                    measure(counts, kind, value, sources);
                }
            }
        }
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn separates_results_and_omits_binary() {
        let rows = estimate(
            &json!({"messages":[{"role":"user","content":[{"type":"text","text":"hello"},{"type":"tool_result","content":"completed"},{"type":"image","source":{"data":"SECRET"}}]}]}),
            &[],
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.kind == Kind::Results)
                .unwrap()
                .items,
            Some(1)
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.kind == Kind::Skills)
                .unwrap()
                .tokens,
            None
        );
        assert!(
            estimate(&json!({"unknown":"private"}), &[])
                .iter()
                .all(|row| row.tokens.is_none())
        );
    }
    #[test]
    fn attributes_only_present_source_fragments() {
        let sources = [
            Source {
                kind: Kind::Injected,
                text: "injected fragment".into(),
            },
            Source {
                kind: Kind::Skills,
                text: "skill fragment".into(),
            },
        ];
        let rows = estimate(
            &json!({"instructions":"base injected fragment skill fragment"}),
            &sources,
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.kind == Kind::Injected)
                .unwrap()
                .items,
            Some(1)
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.kind == Kind::Skills)
                .unwrap()
                .items,
            Some(1)
        );
        let omitted = estimate(&json!({"instructions":"rewritten upstream"}), &sources);
        assert_eq!(
            omitted
                .iter()
                .find(|row| row.kind == Kind::Skills)
                .unwrap()
                .tokens,
            None
        );
    }
}
