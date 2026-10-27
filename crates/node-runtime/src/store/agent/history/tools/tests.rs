use super::*;
use serde_json::json;
mod presentation;
mod recipes;

fn fixture() -> Raw {
    let mut raw = Raw::new(
        Some("call".into()),
        "read".into(),
        json!({
            "path":"文".repeat(900), "body":"PRIVATE BODY".repeat(100_000),
        }),
    );
    raw.result = Some(json!({"text":"ORIGINAL RESULT".repeat(100_000)}));
    raw.presentation = Presentation::Details;
    raw.display = Some(
        serde_json::from_value(json!({
            "label":"Read", "input":{"summary":{"path":"/path"}},
            "output":{"body":{"text":{"source":"result","path":"/text"}}}
        }))
        .unwrap(),
    );
    raw
}

#[test]
fn excludes_raw_values_and_recipes() {
    let raw = fixture();
    let summary = raw.summary();
    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.len() < 16 * 1024);
    assert!(!json.contains("PRIVATE BODY"));
    assert!(!json.contains("ORIGINAL RESULT"));
    assert!(!json.contains("source"));
    assert!(summary.has_details);
    let resolved = summary.resolved.unwrap();
    assert_eq!(
        resolved.input.summary.unwrap().text,
        raw.arguments["path"]
            .as_str()
            .unwrap()
            .chars()
            .take(200)
            .collect::<String>()
    );
    assert!(resolved.output.unwrap().body.is_none());
    let detail = raw.detail();
    assert_eq!(detail.arguments, raw.arguments);
    assert_eq!(detail.result, raw.result);
    assert!(
        detail
            .resolved
            .unwrap()
            .output
            .unwrap()
            .body
            .unwrap()
            .text
            .len()
            > 100_000
    );
}

#[test]
fn bounds_explicit_preview_and_errors() {
    let mut raw = fixture();
    raw.result = Some(json!({
        "text":"visible 🙂".repeat(2000),
        "error":{"message":"failure 中文".repeat(3000)},
        "sailry_result":{"version":1, "diagnostics":[{"text":"diagnostic".repeat(4000),"error":true}]}
    }));
    raw.failure = Some(Failure {
        code: Some(ErrorCode::Unavailable),
    });
    raw.display.as_mut().unwrap().output = Some(
        serde_json::from_value(json!({
            "preview":{"parts":[{"value":{"source":"result","path":"/text"},"take":2048}]}
        }))
        .unwrap(),
    );
    let summary = raw.summary();
    assert_eq!(summary.error.unwrap().chars().count(), 1024);
    let resolved = summary.resolved.unwrap();
    assert!(resolved.output.unwrap().preview.chars().count() <= 2048);
    assert_eq!(resolved.diagnostics[0].text.chars().count(), 256);
    assert!(raw.detail().result.unwrap()["text"].as_str().unwrap().len() > 10_000);
}

#[test]
fn raw_inspection_still_requires_detail() {
    let mut raw = fixture();
    raw.display.as_mut().unwrap().raw_result = true;
    let summary = raw.summary();
    assert!(summary.raw_result && summary.has_details);
    assert!(
        !serde_json::to_string(&summary)
            .unwrap()
            .contains("ORIGINAL RESULT")
    );
}

#[test]
fn revisions_follow_public_detail() {
    let mut raw = fixture();
    let original = raw.summary().revision;
    assert_eq!(original, raw.summary().revision);
    raw.result = Some(json!({"text":"updated"}));
    assert_ne!(original, raw.summary().revision);
}
