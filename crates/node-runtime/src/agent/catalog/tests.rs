use super::*;
use serde_json::json;

#[test]
fn moves_context_without_changing_output() {
    let raw = json!({"data":{"lines":[{"text":"unchanged"}]},"sailry_context":{"loaded_tools":["read_file"]}});
    let result = json(raw).unwrap();
    assert_eq!(
        result.structured_content,
        Some(json!({"data":{"lines":[{"text":"unchanged"}]}}))
    );
    assert_eq!(
        result.meta.unwrap()["sailry_context"]["loaded_tools"],
        json!(["read_file"])
    );
    let raw = json!({"sailry_context":"ordinary marker","output":7});
    assert_eq!(json(raw.clone()).unwrap().structured_content, Some(raw));
    assert!(json(json!({"sailry_context":{"loaded_tools":7}})).is_err());
}

#[test]
fn rejects_anonymous_calls() {
    assert!(
        CallContext::new(
            "session".into(),
            "",
            &CallToolRequestParams::new("write"),
            CancellationToken::new(),
            Default::default()
        )
        .is_err()
    );
}
