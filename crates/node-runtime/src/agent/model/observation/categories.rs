//! Ported from fb70fdc1's model/observation/categories.rs.
//! Estimates follow adapted roles, system fragments and verified context ranges.
use super::{Context, Source, context::Location};
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
pub(super) fn estimate(
    body: &Value,
    inventory: Option<&[Source]>,
    contexts: &[Context],
) -> Vec<Category> {
    let mut counts = Counts::new();
    let sources = inventory.unwrap_or_default();
    let locations: Vec<_> = contexts
        .iter()
        .filter_map(|context| context.locate(body))
        .collect();
    let mut found = BTreeSet::new();
    for field in [
        "instructions",
        "system",
        "systemInstruction",
        "system_instruction",
    ] {
        if let Some(value) = body.get(field) {
            add(
                &mut counts,
                &mut found,
                Kind::System,
                value,
                sources,
                &locations,
            );
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
            add(
                &mut counts,
                &mut found,
                Kind::Tools,
                tool,
                sources,
                &locations,
            );
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
                                &locations,
                            );
                        }
                    } else {
                        add(
                            &mut counts,
                            &mut found,
                            kind,
                            content.unwrap_or(message),
                            sources,
                            &locations,
                        );
                    }
                    // Chat adapters keep tool calls and reasoning beside content,
                    // including null or empty content on tool-only messages.
                    if kind == Kind::Assistant && content.is_some() {
                        for field in ["tool_calls", "function_call", "reasoning_content"] {
                            if let Some(value) = message.get(field) {
                                add(&mut counts, &mut found, kind, value, sources, &locations);
                            }
                        }
                    }
                }
            }
            Some(value @ Value::String(_)) => add(
                &mut counts,
                &mut found,
                Kind::User,
                value,
                sources,
                &locations,
            ),
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
                    }) && contexts.iter().all(|context| {
                        !context.contains(kind)
                            || locations
                                .iter()
                                .any(|location| std::ptr::eq(location.context, context))
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
    locations: &[Location<'_>],
) {
    counts.entry(kind).or_default().0 += 1;
    // System fragments never establish attribution inside ordinary user text.
    // Turn contexts have separately verified nodes and exact byte ranges.
    measure(
        counts,
        found,
        kind,
        value,
        if kind == Kind::System { sources } else { &[] },
        locations,
    );
}
fn measure(
    counts: &mut Counts,
    found: &mut BTreeSet<usize>,
    kind: Kind,
    value: &Value,
    sources: &[Source],
    locations: &[Location<'_>],
) {
    match value {
        Value::String(text) if !text.starts_with("data:") => {
            if kind == Kind::User {
                let mut matched: Vec<_> = locations
                    .iter()
                    .filter(|location| std::ptr::eq(text.as_str(), location.text))
                    .collect();
                matched.sort_by_key(|location| location.start);
                if !matched.is_empty() {
                    let mut end = 0;
                    for location in matched {
                        if location.start < end {
                            continue;
                        }
                        counts.entry(kind).or_default().1 +=
                            text[end..location.start].chars().count() as u64;
                        for (range, source) in &location.context.ranges {
                            let count = counts.entry(*source).or_default();
                            count.0 += 1;
                            count.1 += location.context.text[range.clone()].chars().count() as u64;
                        }
                        end = location.start + location.context.text.len();
                    }
                    counts.entry(kind).or_default().1 += text[end..].chars().count() as u64;
                    return;
                }
            }
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
                measure(counts, found, kind, value, sources, locations);
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
                    measure(counts, found, kind, value, sources, locations);
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
            let rows = estimate(&json!({"contents":[], "tools":tools}), None, &[]);
            let tools = rows.iter().find(|row| row.kind == Kind::Tools).unwrap();
            assert_eq!(tools.items, Some(1));
            assert_eq!(tools.tokens, Some(4));
        }
        let rows = estimate(
            &json!({"contents":[], "tools":{"opaque":"declarations"}}),
            None,
            &[],
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
                &[],
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
            &[],
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
            estimate(&json!({"unknown":"private"}), None, &[])
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
            &[],
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
            &[],
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
        let known = estimate(&body, Some(&[]), &[]);
        let missing = estimate(&body, None, &[]);
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
        let rows = estimate(&quoted, Some(&sources), &[]);
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
        let rows = estimate(
            &json!({"instructions":"skill fragment"}),
            Some(&sources),
            &[],
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.kind == Kind::Skills)
                .unwrap()
                .tokens,
            None
        );
        assert!(
            estimate(&json!({"input":null}), Some(&[]), &[])
                .iter()
                .all(|row| row.tokens.is_none())
        );
    }

    #[test]
    fn scopes_terminal_fragments() {
        let context = Context::new(vec![
            Source {
                kind: Kind::Injected,
                text: "Current state 中文 🙂".into(),
            },
            Source {
                kind: Kind::Skills,
                text: "Exact skill body".into(),
            },
        ])
        .unwrap();
        let sources = [Source {
            kind: Kind::Injected,
            text: "Policy fragment".into(),
        }];
        let quotation = "Quote Policy fragment and Exact skill body; <turn-context>\nOlder state\n</turn-context>";
        let bodies = [
            json!({"messages":[{"role":"system","content":"Base Policy fragment"},
                {"role":"user","content":format!("{quotation}\n{}",context.text())}]}),
            json!({"input":[{"role":"system","content":[{"type":"input_text","text":"Base Policy fragment"}]},
                {"role":"user","content":[{"type":"input_text","text":quotation},{"type":"input_text","text":context.text()}]}]}),
            json!({"system":"Base Policy fragment","messages":[{"role":"user","content":[
                {"type":"text","text":quotation},{"type":"text","text":context.text()}]}]}),
            json!({"systemInstruction":{"parts":[{"text":"Base Policy fragment"}]},"contents":[{"role":"user","parts":[
                {"text":quotation},{"text":context.text()}]}]}),
        ];
        for body in bodies {
            let original = body.clone();
            let rows = estimate(&body, Some(&sources), std::slice::from_ref(&context));
            let tokens = |kind| rows.iter().find(|row| row.kind == kind).unwrap().tokens;
            assert_eq!(tokens(Kind::Skills), Some(4));
            assert_eq!(
                tokens(Kind::Injected),
                Some(
                    ("Policy fragment".chars().count() as u64
                        + context.text().chars().count() as u64
                        - "Exact skill body".chars().count() as u64)
                        .div_ceil(4)
                )
            );
            assert!(tokens(Kind::User).unwrap() >= (quotation.chars().count() as u64).div_ceil(4));
            assert_eq!(body, original);
        }
    }

    #[test]
    fn rejects_rewritten_context() {
        let context = Context::new(vec![Source {
            kind: Kind::Skills,
            text: "Full skill".into(),
        }])
        .unwrap();
        for body in [
            json!({"system":"base","messages":[{"role":"user","content":"rewritten"}]}),
            json!({"system":"base","messages":[{"role":"user","content":context.text()},{"role":"user","content":"later input"}]}),
            json!({"system":"base","messages":[{"role":"user","content":format!("{} altered",context.text())}]}),
            json!({"system":"base","messages":[{"role":"user","content":[{"type":"text","text":context.text()},{"type":"image","data":"opaque"}]}]}),
            json!({"system":"base","messages":[],"input":[{"role":"user","content":context.text()}]}),
            json!({"input":{"opaque":context.text()}}),
        ] {
            let rows = estimate(&body, Some(&[]), std::slice::from_ref(&context));
            for kind in [Kind::Skills, Kind::Injected] {
                assert_eq!(
                    rows.iter().find(|row| row.kind == kind).unwrap().tokens,
                    None,
                    "{body}"
                );
            }
        }
    }

    #[test]
    fn counts_persisted_context_without_reinjection() {
        let skills = vec![
            "First complete skill".to_owned(),
            "Second complete skill".to_owned(),
        ];
        let contexts: Vec<_> = skills
            .iter()
            .map(|skill| {
                let text = Context::new(vec![Source {
                    kind: Kind::Skills,
                    text: skill.clone(),
                }])
                .unwrap();
                Context::persisted(text.text().to_owned(), &skills)
            })
            .collect();
        let quoted = "User quotes First complete skill";
        let joined = format!(
            "{quoted}\n{}\n{}\nNew task",
            contexts[0].text(),
            contexts[1].text()
        );
        let bodies = [
            json!({"messages":[{"role":"user","content":joined},{"role":"assistant","content":"reply"}]}),
            json!({"input":[{"role":"user","content":[{"type":"input_text","text":quoted},{"type":"input_text","text":contexts[0].text()},{"type":"input_text","text":contexts[1].text()}]},{"role":"assistant","content":"reply"}]}),
            json!({"messages":[{"role":"user","content":[{"type":"text","text":quoted},{"type":"text","text":contexts[0].text()},{"type":"text","text":contexts[1].text()}]},{"role":"assistant","content":"reply"}]}),
            json!({"contents":[{"role":"user","parts":[{"text":quoted},{"text":contexts[0].text()},{"text":contexts[1].text()}]},{"role":"model","parts":[{"text":"reply"}]}]}),
        ];
        for body in bodies {
            let rows = estimate(&body, Some(&[]), &contexts);
            let tokens = |kind| rows.iter().find(|row| row.kind == kind).unwrap().tokens;
            assert_eq!(
                tokens(Kind::Skills),
                Some(
                    (skills
                        .iter()
                        .map(|text| text.chars().count() as u64)
                        .sum::<u64>())
                    .div_ceil(4)
                )
            );
            assert_eq!(
                tokens(Kind::Injected),
                Some(
                    (contexts
                        .iter()
                        .map(|context| context.text().chars().count() as u64)
                        .sum::<u64>()
                        - skills
                            .iter()
                            .map(|text| text.chars().count() as u64)
                            .sum::<u64>())
                    .div_ceil(4)
                )
            );
            assert!(tokens(Kind::User).unwrap() >= (quoted.chars().count() as u64).div_ceil(4));
        }
    }

    #[test]
    fn rejects_ambiguous_persisted_context() {
        let skill = "Captured skill".to_owned();
        let context = Context::new(vec![Source {
            kind: Kind::Skills,
            text: skill.clone(),
        }])
        .unwrap();
        let context = Context::persisted(context.text().to_owned(), &[skill]);
        for body in [
            json!({"messages":[{"role":"user","content":context.text()},{"role":"user","content":context.text()}]}),
            json!({"messages":[{"role":"user","content":format!("{}\n{}", context.text(), context.text())}]}),
            json!({"messages":[{"role":"user","content":"rewritten"}]}),
            json!({"messages":[{"role":"user","content":format!("quoted {}", context.text())}]}),
        ] {
            let rows = estimate(&body, Some(&[]), std::slice::from_ref(&context));
            for kind in [Kind::Skills, Kind::Injected] {
                assert_eq!(
                    rows.iter().find(|row| row.kind == kind).unwrap().tokens,
                    None
                );
            }
        }
    }
}
