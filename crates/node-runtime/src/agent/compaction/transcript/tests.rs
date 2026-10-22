use super::*;
use adk_core::{Content, FunctionResponseData};

#[test]
fn preserves_facts_without_reasoning() {
    let args = json!({"path":"src/main.rs", "text":"Exact argument ".repeat(300)});
    let mut event = Event::new("fixture");
    event.author = "assistant".into();
    event.set_content(Content {
        role: "model".into(),
        parts: vec![
            Part::Thinking {
                thinking: "private reasoning".repeat(1000),
                signature: Some("signed reasoning".into()),
            },
            Part::Text {
                text: "Changed src/main.rs".into(),
            },
            Part::FunctionCall {
                name: "write_file".into(),
                args: args.clone(),
                id: Some("write-1".into()),
                thought_signature: Some("signed call".into()),
            },
            Part::FunctionResponse {
                id: Some("write-1".into()),
                function_response: FunctionResponseData::new(
                    "write_file",
                    json!({"error":"permission denied", "path":"src/main.rs"}),
                ),
                annotations: None,
            },
        ],
    });
    let canonical = serde_json::to_vec(&event).unwrap();
    let text = encode(std::slice::from_ref(&event)).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value[0]["parts"].as_array().unwrap().len(), 3);
    assert_eq!(value[0]["parts"][1]["function_call"]["args"], args);
    assert!(text.contains("permission denied"));
    assert!(text.contains("write-1"));
    assert!(!text.contains("reasoning"));
    assert!(!text.contains("signed call"));
    assert!(text.len() < canonical.len());
    assert_eq!(serde_json::to_vec(&event).unwrap(), canonical);
}

#[test]
fn bounds_unicode_results_with_explicit_marker() {
    let original = json!({"error":"read failed", "text":"证据🙂".repeat(1500)});
    let value = result(&original);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["error"], true);
    let text = value["text"].as_str().unwrap();
    let prefix: String = original
        .to_string()
        .chars()
        .take(TOOL_RESULT_CHARS)
        .collect();
    assert!(text.starts_with(&prefix));
    assert_eq!(prefix.chars().count(), TOOL_RESULT_CHARS);
    assert!(text.contains("more characters truncated]"));
    let small = json!({"error":"read failed", "path":"src/main.rs"});
    assert_eq!(result(&small), small);
}

#[test]
fn retains_attribution_after_omitting_parts() {
    let mut event = Event::new("fixture");
    event.author = "user".into();
    event
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "user".into());
    event
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "1".into());
    event.set_content(Content {
        role: "user".into(),
        parts: vec![
            Part::Thinking {
                thinking: "omitted".into(),
                signature: None,
            },
            Part::Text {
                text: "Keep this requirement".into(),
            },
            Part::InlineData {
                mime_type: "image/png".into(),
                data: vec![255; 1000],
                uri: Some("attachment:screen".into()),
                annotations: None,
            },
        ],
    });
    let text = encode(&[event]).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value[0]["original_text_part"], 0);
    assert_eq!(value[0]["input_source"], "user");
    assert_eq!(value[0]["parts"][1]["uri"], "attachment:screen");
    assert_eq!(value[0]["parts"][1]["contents_omitted"], true);
    assert!(!text.contains("255"));
}
