//! Ported from fb70fdc1's model/observation/categories.rs.
//! Estimates follow adapted payload roles and exact Node-owned system fragments.
use super::Source;
use sailry_protocol::conversation::observation::{Category, CategoryKind as Kind};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type Counts = BTreeMap<Kind, (u64, u64)>;

pub(super) fn retains_system(body: &Value, expected: blake3::Hash) -> bool {
    let mut parts: Vec<_> = [
        "instructions",
        "system",
        "systemInstruction",
        "system_instruction",
    ]
    .iter()
    .filter_map(|field| body.get(field))
    .collect();
    for field in ["messages", "contents", "input"] {
        for message in body
            .get(field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if matches!(
                message.get("role").and_then(Value::as_str),
                Some("system" | "developer")
            ) {
                parts.extend(message.get("content").or_else(|| message.get("parts")));
            }
        }
    }
    if parts.is_empty() {
        return expected == blake3::hash(b"");
    }
    parts.iter().any(|value| contains(value, expected))
}

fn contains(value: &Value, expected: blake3::Hash) -> bool {
    match value {
        Value::String(text) => blake3::hash(text.as_bytes()) == expected,
        Value::Array(values) => values.iter().any(|value| contains(value, expected)),
        Value::Object(values) => values
            .iter()
            .filter(|(key, _)| matches!(key.as_str(), "text" | "parts" | "content"))
            .any(|(_, value)| contains(value, expected)),
        _ => false,
    }
}
pub(super) fn estimate(body: &Value, inventory: Option<&[Source]>) -> Vec<Category> {
    let mut counts = Counts::new();
    let sources = inventory.unwrap_or_default();
    let mut found = BTreeSet::new();
    for field in [
        "instructions",
        "system",
        "systemInstruction",
        "system_instruction",
    ] {
        if let Some(value) = body.get(field) {
            add(&mut counts, &mut found, Kind::System, value, sources);
        }
    }
    // Gemini's GDK adapter wraps declarations in an object; other adapters
    // send an array of tools, including Gemini-compatible package adapters.
    let tools = body.get("tools");
    let declarations = tools.and_then(|tools| {
        tools.as_array().or_else(|| {
            tools
                .get("functionDeclarations")
                .or_else(|| tools.get("function_declarations"))
                .and_then(Value::as_array)
        })
    });
    if let Some(tools) = declarations {
        for tool in tools {
            add(&mut counts, &mut found, Kind::Tools, tool, sources);
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
                                &mut found,
                                if result { Kind::Results } else { kind },
                                part,
                                sources,
                            );
                        }
                    } else {
                        add(
                            &mut counts,
                            &mut found,
                            kind,
                            content.unwrap_or(message),
                            sources,
                        );
                    }
                    // Chat adapters keep tool calls and reasoning beside content,
                    // including null or empty content on tool-only messages.
                    if kind == Kind::Assistant && content.is_some() {
                        for field in ["tool_calls", "function_call", "reasoning_content"] {
                            if let Some(value) = message.get(field) {
                                add(&mut counts, &mut found, kind, value, sources);
                            }
                        }
                    }
                }
            }
            Some(value @ Value::String(_)) => {
                add(&mut counts, &mut found, Kind::User, value, sources)
            }
            _ => {}
        }
    }
    let recognized = [
        "instructions",
        "system",
        "systemInstruction",
        "system_instruction",
    ]
    .iter()
    .any(|field| {
        body.get(field).is_some_and(|value| {
            matches!(value, Value::Array(_) | Value::Object(_) | Value::String(_))
        })
    }) || ["messages", "contents", "input"]
        .iter()
        .any(|field| body.get(field).is_some_and(Value::is_array))
        || body.get("input").is_some_and(Value::is_string);
    let supported = ["messages", "contents", "input"].iter().all(|field| {
        body.get(field)
            .is_none_or(|value| value.is_array() || (*field == "input" && value.is_string()))
    });
    let recognized = recognized
        && supported
        && tools.is_none_or(|tools| tools.is_null() || declarations.is_some());
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
        // A complete current prompt inventory establishes an absent group as
        // zero. Rewritten, omitted or unavailable provenance remains unknown.
        let identified = matches!(kind, Kind::Skills | Kind::Injected);
        let known = recognized
            && (!identified
                || inventory.is_some_and(|sources| {
                    sources.iter().enumerate().all(|(index, source)| {
                        source.kind != kind || source.text.is_empty() || found.contains(&index)
                    })
                }));
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
fn add(
    counts: &mut Counts,
    found: &mut BTreeSet<usize>,
    kind: Kind,
    value: &Value,
    sources: &[Source],
) {
    counts.entry(kind).or_default().0 += 1;
    // These fragments were supplied as system prompt parts. Matching a user's
    // quotation or an old tool result does not establish system attribution.
    measure(
        counts,
        found,
        kind,
        value,
        if kind == Kind::System { sources } else { &[] },
    );
}
fn measure(
    counts: &mut Counts,
    found: &mut BTreeSet<usize>,
    kind: Kind,
    value: &Value,
    sources: &[Source],
) {
    match value {
        Value::String(text) if !text.starts_with("data:") => {
            let mut ranges = Vec::new();
            for (index, source) in sources.iter().enumerate() {
                if source.text.is_empty() {
                    continue;
                }
                for (start, matched) in text.match_indices(&source.text) {
                    ranges.push((start, start + matched.len(), source.kind, index));
                }
            }
            ranges.sort_by_key(|&(start, end, _, _)| (start, std::cmp::Reverse(end)));
            let mut until = 0;
            let mut attributed = 0;
            for (start, end, source, index) in ranges {
                if start < until {
                    continue;
                }
                until = end;
                found.insert(index);
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
                measure(counts, found, kind, value, sources);
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
                    measure(counts, found, kind, value, sources);
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
    fn counts_wrapped_tool_declarations() {
        let declaration = json!({
            "name":"read", "description":"file", "parameters":{"type":"object"}
        });
        for tools in [
            json!([declaration.clone()]),
            json!({"functionDeclarations":[declaration.clone()]}),
            json!({"function_declarations":[declaration]}),
        ] {
            let rows = estimate(&json!({"contents":[], "tools":tools}), None);
            let tools = rows.iter().find(|row| row.kind == Kind::Tools).unwrap();
            assert_eq!(tools.items, Some(1));
            assert_eq!(tools.tokens, Some(4));
        }
        let rows = estimate(
            &json!({"contents":[], "tools":{"opaque":"declarations"}}),
            None,
        );
        assert!(rows.iter().all(|row| row.tokens.is_none()));
    }

    #[test]
    fn counts_assistant_siblings() {
        for (content, characters) in [
            (Value::Null, 0),
            (json!(""), 0),
            (json!("reply"), 5),
            (json!([{"type":"text", "text":"reply"}]), 9),
        ] {
            let rows = estimate(
                &json!({"messages":[{
                    "role":"assistant",
                    "content":content,
                    "tool_calls":[{"id":"call", "type":"function", "function":{
                        "name":"read", "arguments":"{}"
                    }}],
                    "reasoning_content":"think"
                }]}),
                None,
            );
            assert_eq!(
                rows.iter()
                    .find(|row| row.kind == Kind::Assistant)
                    .unwrap()
                    .tokens,
                Some((23_u64 + characters).div_ceil(4)),
            );
        }
        // The whole-message fallback already includes its sibling fields once.
        let rows = estimate(
            &json!({"messages":[{
                "role":"assistant",
                "function_call":{"name":"read", "arguments":"{}"},
                "reasoning_content":"think"
            }]}),
            None,
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.kind == Kind::Assistant)
                .unwrap()
                .tokens,
            Some(5),
        );
    }

    #[test]
    fn separates_results_and_omits_binary() {
        let rows = estimate(
            &json!({"messages":[{"role":"user","content":[{"type":"text","text":"hello"},{"type":"tool_result","content":"completed"},{"type":"image","source":{"data":"SECRET"}}]}]}),
            None,
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
            estimate(&json!({"unknown":"private"}), None)
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
            Some(&sources),
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
        let omitted = estimate(
            &json!({"instructions":"rewritten upstream"}),
            Some(&sources),
        );
        assert_eq!(
            omitted
                .iter()
                .find(|row| row.kind == Kind::Skills)
                .unwrap()
                .tokens,
            None
        );
    }

    #[test]
    fn distinguishes_empty_and_missing_provenance() {
        let body = json!({"instructions":"base","input":[]});
        let known = estimate(&body, Some(&[]));
        let missing = estimate(&body, None);
        for kind in [Kind::Skills, Kind::Injected] {
            assert_eq!(
                known.iter().find(|row| row.kind == kind).unwrap().tokens,
                Some(0)
            );
            assert_eq!(
                missing.iter().find(|row| row.kind == kind).unwrap().tokens,
                None
            );
        }
        let quoted =
            json!({"instructions":"base","input":[{"role":"user","content":"skill fragment"}]});
        let sources = [Source {
            kind: Kind::Skills,
            text: "skill fragment".into(),
        }];
        let rows = estimate(&quoted, Some(&sources));
        assert_eq!(
            rows.iter()
                .find(|row| row.kind == Kind::Skills)
                .unwrap()
                .tokens,
            None
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.kind == Kind::User)
                .unwrap()
                .tokens,
            Some(4)
        );
        let mut sources = sources.to_vec();
        sources.push(Source {
            kind: Kind::Skills,
            text: "missing fragment".into(),
        });
        let rows = estimate(&json!({"instructions":"skill fragment"}), Some(&sources));
        assert_eq!(
            rows.iter()
                .find(|row| row.kind == Kind::Skills)
                .unwrap()
                .tokens,
            None
        );
        assert!(
            estimate(&json!({"input":null}), Some(&[]))
                .iter()
                .all(|row| row.tokens.is_none())
        );
    }
}
