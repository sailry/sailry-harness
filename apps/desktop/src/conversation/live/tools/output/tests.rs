use super::*;
use crate::conversation::surface::literal;
use core::prelude::v1::test;
use sailry_protocol::{
    Fault, FileLine, FileLines, Output, SearchMatch, SearchResults,
    tool::{Origin, projection::Output as Projection},
};
use serde_json::json;

fn file_projection(name: &str) -> Projection {
    let manifest: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/files/plugin.json"
    )))
    .unwrap();
    let declaration = manifest["extensions"]["dev.sailry.platform"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap();
    let projection: Projection =
        serde_json::from_value(declaration["display"]["output"].clone()).unwrap();
    assert!(projection.valid());
    projection
}

#[test]
fn uses_the_canonical_message_for_every_origin() {
    let message = "No running application matches 'Terminal'";
    for origin in [Origin::Runtime, Origin::Tool] {
        let observed = Failure { code: None, origin };
        let summary = Summary {
            failure: Some(observed.clone()),
            diagnostics: true,
            error: Some(message.into()),
            ..Default::default()
        };
        for value in [
            json!({"error": {"code": "APP_NOT_FOUND", "message": message}}),
            json!({
                "isError": true,
                "content": [{
                    "type": "text",
                    "text": json!({
                        "error": {"code": "APP_NOT_FOUND", "message": message},
                        "suggestion": "Open an application first",
                        "retry": true
                    }).to_string()
                }]
            }),
            json!({"code": -32602, "message": message}),
            json!({"error": "A different raw diagnostic"}),
        ] {
            let original = value.clone();
            assert_eq!(failure(Some(&summary)), Some(message));
            let body = tool_preview(Some(&observed), Some(&summary), &value);
            assert_eq!(body.text, message);
            assert_eq!(body.copy_text(), message);
            assert!(body.notices.is_empty());
            assert_eq!(
                serde_json::from_str::<Value>(&raw(&value)).unwrap(),
                original
            );
            assert_eq!(value, original);
        }
    }
}

#[test]
fn diagnostics_preserve_message_content() {
    for message in [
        "Condition not met",
        "Condition diagnostic\nWorker disconnected",
        "Input failed\n[detail](literal) 中文\n  Original diagnostic  ",
        "Before-tool hook failed\nLiteral [diagnostic] 中文 🙂",
        "Database query failed: syntax error near quoted_table",
        "file already exists: assets/image.png\nChoose another output path",
    ] {
        let summary = Summary {
            error: Some(message.into()),
            diagnostics: true,
            ..Default::default()
        };
        for observed in [
            None,
            Some(Failure::runtime(Some(ErrorCode::Unavailable))),
            Some(Failure::tool(Some(ErrorCode::InvalidRequest))),
            Some(Failure::runtime(Some(ErrorCode::Cancelled))),
            Some(Failure::runtime(Some(ErrorCode::OutcomeUnknown))),
        ] {
            let body = tool_preview(observed.as_ref(), Some(&summary), &Value::Null);
            assert_eq!(failure(Some(&summary)), Some(message));
            assert_eq!(body.text, message);
            assert_eq!(body.copy_text(), message);
            assert!(body.notices.is_empty());
        }
    }
    assert!(failure(None).is_none());
    assert!(failure(Some(&Summary::default())).is_none());
    assert!(
        failure(Some(&Summary {
            error: Some(" \n ".into()),
            ..Default::default()
        }))
        .is_none()
    );
}

#[test]
fn missing_messages_never_expose_error_envelopes() {
    for observed in [Failure::runtime(None), Failure::tool(None)] {
        let summary = Summary {
            failure: Some(observed.clone()),
            diagnostics: true,
            ..Default::default()
        };
        for value in [
            json!({"isError": true, "content": []}),
            json!({"isError": true, "content": [{"type": "image", "text": "Not diagnostic text"}]}),
            json!({"isError": true, "content": [{"type": "text", "text": false}]}),
            json!({"isError": true, "content": [{"type": "text", "text": "{\"code\":\"APP_NOT_FOUND\"}"}]}),
            json!({"output": {"isError": true, "error": "domain row"}}),
            json!({"error": Fault::new(ErrorCode::Unavailable, "Raw diagnostic")}),
            json!({"sailry_result": {"version": 1, "diagnostics": [{"text": "Raw diagnostic", "error": true}]}}),
            json!("{\"code\":\"APP_NOT_FOUND\"}"),
            json!(["{\"code\":\"APP_NOT_FOUND\"}"]),
        ] {
            let body = tool_preview(Some(&observed), Some(&summary), &value);
            assert!(body.text.is_empty());
            assert_eq!(body.notices, ["tool_failed"]);
            assert_eq!(serde_json::from_str::<Value>(&raw(&value)).unwrap(), value);
        }
    }
}

#[test]
fn business_fields_do_not_establish_failures() {
    for value in [
        json!({"isError": false, "structuredContent": {"isError": true, "error": "domain row"}}),
        json!({"output": {"isError": true, "error": "domain row"}}),
        json!({"isError": true, "error": "domain row"}),
        json!({"isError": "true", "content": [{"type": "text", "text": "Literal content"}]}),
        json!({"error": Fault::new(ErrorCode::Unavailable, "domain row")}),
        json!({"response": {"error": "input failed"}}),
        json!({"error": "literal", "sailry_result": {"version": 2, "diagnostics": []}}),
        json!({"error": "literal", "sailry_result": {"version": 1, "diagnostics": [{"text": "user data", "error": "yes"}]}}),
        Value::Null,
    ] {
        let original = value.clone();
        let body = tool_preview(None, None, &value);
        assert!(body.text.is_empty());
        assert!(body.notices.is_empty());
        assert_eq!(
            serde_json::from_str::<Value>(&raw(&value)).unwrap(),
            original
        );
        assert_eq!(value, original);
    }
}

#[test]
fn missing_messages_keep_typed_notices() {
    for (code, expected) in [
        (ErrorCode::NotFound, "tool_not_found"),
        (ErrorCode::InvalidRequest, "tool_invalid"),
        (ErrorCode::PermissionDenied, "tool_denied"),
        (ErrorCode::Cancelled, "turn_cancelled"),
        (ErrorCode::OutcomeUnknown, "tool_interrupted"),
        (ErrorCode::Unavailable, "tool_unavailable"),
        (ErrorCode::Internal, "tool_failed"),
    ] {
        for origin in [Origin::Runtime, Origin::Tool] {
            let observed = Failure {
                code: Some(code),
                origin,
            };
            let value = json!({"error": Fault::new(code, "Raw diagnostic")});
            let body = tool_preview(Some(&observed), None, &value);
            assert!(body.text.is_empty());
            assert_eq!(body.notices, [expected]);
            assert_eq!(body.copy_text(), crate::tr(expected).to_string());
        }
    }
}

#[test]
fn literal_success_output_stays_unchanged() {
    for (value, text) in [
        (
            json!("Original [output] 中文 🙂"),
            "Original [output] 中文 🙂",
        ),
        (json!(["first", "second"]), "first\nsecond"),
        (json!(42), "42"),
        (json!(false), "false"),
    ] {
        let body = tool_preview(None, None, &value);
        assert_eq!(body.text, text);
        assert_eq!(body.copy_text(), text);
        assert!(body.notices.is_empty());
    }
}

#[test]
fn file_output_keeps_content_and_limits() {
    let source = "中文 🙂\n[content](file)\n```rust\nlet value = 1;\n```";
    let result = serde_json::to_value(Output::FileLines(FileLines {
        path: "source.rs".into(),
        lines: source
            .split_inclusive('\n')
            .enumerate()
            .map(|(index, text)| FileLine {
                number: index + 1,
                text: text.into(),
            })
            .collect(),
        size: 200,
        revision: "full-content-revision".into(),
        offset: 0,
        next_offset: Some(5),
        truncated: false,
    }))
    .unwrap();
    let projection = file_projection("read_file");
    let arguments = json!({"path": "source.rs"});
    let text = projection
        .preview
        .as_ref()
        .unwrap()
        .render(&arguments, &result);
    assert_eq!(
        text,
        "1: 中文 🙂\n2: [content](file)\n3: ```rust\n4: let value = 1;\n5: ```"
    );
    assert_eq!(
        projection.notices(&arguments, &result, "en"),
        ["Partial results"]
    );
    let body = preview(None, &result);
    assert!(body.text.is_empty());
    assert_eq!(
        serde_json::from_str::<Value>(&raw(&result)).unwrap(),
        result
    );
    assert!(body.notices.is_empty());
    assert!(literal(&text).starts_with("````\n"));
    assert!(literal(&text).ends_with("\n````"));
    let mut final_page = result.clone();
    final_page["data"]["next_offset"] = json!(null);
    assert!(projection.notices(&arguments, &final_page, "en").is_empty());
    final_page["data"]["offset"] = json!(1);
    assert_eq!(
        projection.notices(&arguments, &final_page, "en"),
        ["Partial results"]
    );
}

#[test]
fn search_output_keeps_partial_notices() {
    let result = serde_json::to_value(Output::SearchResults(SearchResults {
        matches: vec![SearchMatch {
            path: "资料.txt".into(),
            line_number: 3,
            line: "matched text".into(),
            matched: true,
            truncated: false,
        }],
        scanned_files: 2,
        skipped: 1,
        truncated: false,
        ..Default::default()
    }))
    .unwrap();
    let projection = file_projection("search_files");
    let arguments = json!({"query": "matched text"});
    assert_eq!(
        projection
            .preview
            .as_ref()
            .unwrap()
            .render(&arguments, &result),
        "资料.txt:3  matched text"
    );
    assert_eq!(
        projection.notices(&arguments, &result, "en"),
        ["Partial results"]
    );
    let body = preview(None, &result);
    assert!(body.text.is_empty());
    assert_eq!(
        serde_json::from_str::<Value>(&raw(&result)).unwrap(),
        result
    );
    assert!(body.notices.is_empty());
}

#[test]
fn file_writes_keep_declared_body() {
    let result = serde_json::to_value(Output::FileWritten(sailry_protocol::FileWritten {
        path: "资料.txt".into(),
        revision: "opaque-revision".into(),
        size: 3,
        excerpt: None,
    }))
    .unwrap();
    let projection = file_projection("write_file");
    let arguments = json!({"path": "资料.txt", "text": "文", "expected_revision": null});
    let declared = projection.body.as_ref().unwrap();
    assert_eq!(declared.text.read(&arguments, &result), Some(&json!("文")));
    assert_eq!(
        declared.path.as_ref().unwrap().read(&arguments, &result),
        Some(&json!("资料.txt"))
    );
    assert!(projection.notices(&arguments, &result, "en").is_empty());
    let body = preview(None, &result);
    assert!(body.text.is_empty());
    assert_eq!(
        serde_json::from_str::<Value>(&raw(&result)).unwrap(),
        result
    );
    assert!(body.notices.is_empty());
}
