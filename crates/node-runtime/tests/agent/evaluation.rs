//! Assert Node history independently of workflow resource and outcome assertions.
//! Argument and word matching retain the former evaluation criteria, adapted from
//! ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780, adk-eval/src/{schema,scoring}.rs.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
use sailry_protocol::conversation::{Page, Part};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) async fn trajectory(
    client: &sailry_client::Client,
    page: &Page,
    expected: &[(String, Value)],
) {
    let mut actual = Vec::new();
    for entry in page
        .entries
        .iter()
        .filter(|entry| entry.author != "plan_review")
    {
        for (index, part) in entry.parts.iter().enumerate() {
            if let Part::ToolCall { name, .. } = part {
                actual.push((
                    name.clone(),
                    super::tool_detail(client, page, entry, index)
                        .await
                        .arguments,
                ));
            }
        }
    }
    assert_eq!(
        expected.len(),
        actual.len(),
        "tool trajectory mismatch: expected={expected:?}, actual={actual:?}"
    );
    for ((name, args), (actual_name, actual_args)) in expected.iter().zip(&actual) {
        assert_eq!(name, actual_name, "tool order differs");
        assert!(
            args.is_null() || arguments(args, actual_args),
            "tool arguments differ for {name}: expected={args}, actual={actual_args}"
        );
    }
}

pub(super) fn response(page: &Page, expected: &str) {
    let actual = page
        .entries
        .iter()
        .filter(|entry| entry.author != "user")
        .map(|entry| {
            entry
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        words(expected),
        words(&actual),
        "response mismatch: {actual}"
    );
}

fn arguments(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Number(expected), Value::Number(actual)) => {
            if let (Some(expected), Some(actual)) = (expected.as_i64(), actual.as_i64()) {
                expected == actual
            } else if let (Some(expected), Some(actual)) = (expected.as_u64(), actual.as_u64()) {
                expected == actual
            } else {
                expected.as_f64() == actual.as_f64()
            }
        }
        (Value::Object(expected), Value::Object(actual)) => {
            expected.len() == actual.len()
                && expected.iter().all(|(key, value)| {
                    actual
                        .get(key)
                        .is_some_and(|actual| arguments(value, actual))
                })
        }
        (Value::Array(expected), Value::Array(actual)) => {
            expected.len() == actual.len()
                && expected
                    .iter()
                    .zip(actual)
                    .all(|(expected, actual)| arguments(expected, actual))
        }
        _ => expected == actual,
    }
}

fn words(text: &str) -> BTreeSet<String> {
    let mut words = BTreeSet::new();
    let mut word = String::new();
    for character in text.to_lowercase().chars() {
        let cjk = matches!(character,
            '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' | '\u{f900}'..='\u{faff}'
            | '\u{3040}'..='\u{309f}' | '\u{30a0}'..='\u{30ff}' | '\u{ac00}'..='\u{d7af}'
        );
        if (character.is_whitespace() || cjk) && !word.is_empty() {
            words.insert(std::mem::take(&mut word));
        }
        if cjk {
            words.insert(character.to_string());
        } else if !character.is_whitespace() {
            word.push(character);
        }
    }
    if !word.is_empty() {
        words.insert(word);
    }
    words
}

#[test]
fn matches_numeric_arguments() {
    use serde_json::json;
    assert!(arguments(
        &json!({"items":[1,2]}),
        &json!({"items":[1.0,2.0]})
    ));
    assert!(!arguments(
        &json!({"items":[1]}),
        &json!({"items":[1],"extra":true})
    ));
    assert!(!arguments(&json!({"item":null}), &json!({"item":1})));
}

#[test]
fn matches_word_sets() {
    assert_eq!(words("Complete answer"), words("ANSWER complete complete"));
    assert_eq!(words("完成 answer"), words("answer 成完"));
    assert_ne!(words("Complete answer"), words("Complete answer!"));
}

#[test]
fn preserves_word_boundaries() {
    use sailry_protocol::{
        SessionId, TurnId,
        conversation::{Entry, Queue},
    };
    let mut entry = Entry {
        sequence: 1,
        id: "streamed-answer".into(),
        turn: TurnId::new(),
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 0,
        parts: "Complete answer"
            .chars()
            .map(|ch| Part::Text(ch.to_string()))
            .collect(),
        citations: vec![],
        search_suggestions: None,
        usage: None,
    };
    entry
        .parts
        .insert(4, Part::Thinking("Internal reasoning".into()));
    response(
        &Page {
            session: SessionId::new(),
            revision: 1,
            entries: vec![entry],
            runs: vec![],
            queue: Queue::default(),
            approvals: vec![],
            questions: vec![],
            children: vec![],
            background: vec![],
            next_before: None,
        },
        "Complete answer",
    );
}
