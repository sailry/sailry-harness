use super::*;
use crate::conversation::surface::literal;
use core::prelude::v1::test;
use sailry_protocol::tool::projection::Output as Projection;

const FAILED: Failure = Failure { code: None };
use sailry_protocol::{ErrorCode, Output, SearchMatch, SearchResults};
use sailry_protocol::{FileLine, FileLines};
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
fn declared_diagnostics_keep_their_tone_and_text() {
    let diagnostic = "Verification did not confirm the requested state\n[detail](literal) 中文";
    let result = json!({"response":{"unrelated":"opaque output"},"inline_data":["image bytes"],
        "sailry_result":{"version":1,"diagnostics":[{"text":diagnostic,"error":false}],
            "status":{"label":"Condition not met"}}});
    assert!(!is_error(None, Some(&result)));
    assert_eq!(failure(None, Some(&result)).as_deref(), Some(diagnostic));
    assert_eq!(
        ResultDisplay::from_value(&result)
            .unwrap()
            .status
            .unwrap()
            .label,
        "Condition not met"
    );
    let body = tool_preview(None, &result);
    assert_eq!(body.text, diagnostic);
    assert_eq!(body.copy_text(), diagnostic);
    assert!(body.notices.is_empty());
}

#[test]
fn declared_status_cannot_hide_errors() {
    let marker = json!({"version":1,"diagnostics":[{"text":"Condition diagnostic","error":false}],
        "status":{"label":"Condition not met"}});
    for mut result in [
        json!({"isError":true}),
        json!({"output":{"isError":true}}),
        json!({"error":Fault::new(ErrorCode::Unavailable,"Worker disconnected")}),
    ] {
        result["sailry_result"] = marker.clone();
        assert!(is_error(Some(&FAILED), Some(&result)));
        let diagnostic = if fault(Some(&FAILED), Some(&result)).is_some() {
            "Worker disconnected"
        } else {
            "Condition diagnostic"
        };
        let body = tool_preview(Some(&FAILED), &result);
        assert_eq!(body.text, diagnostic);
        assert_eq!(body.copy_text(), diagnostic);
        assert!(body.notices.is_empty());
    }
    let result = json!({"sailry_result":{"version":1,"diagnostics":[
        {"text":"Condition diagnostic","error":false},{"text":"Worker disconnected","error":true}],
        "status":{"label":"Condition not met"}}});
    assert!(!is_error(None, Some(&result)));
    assert!(is_error(Some(&FAILED), Some(&result)));
    assert_eq!(
        tool_preview(None, &result).copy_text(),
        "Condition diagnostic\nWorker disconnected"
    );
}

#[test]
fn cancellation_and_uncertainty_are_not_execution_errors() {
    for code in [
        ErrorCode::Cancelled,
        ErrorCode::OutcomeUnknown,
        ErrorCode::Unavailable,
        ErrorCode::InvalidRequest,
    ] {
        let observed = Failure { code: Some(code) };
        let result = json!({"isError":true,"error":Fault::new(code,"Diagnostic retained"),
            "sailry_result":{"version":1,"diagnostics":[],"status":{"label":"Complete"}}});
        assert_eq!(
            is_error(Some(&observed), Some(&result)),
            matches!(code, ErrorCode::Unavailable | ErrorCode::InvalidRequest)
        );
        assert_eq!(
            failure(Some(&observed), Some(&result)).as_deref(),
            Some("Diagnostic retained")
        );
        let body = tool_preview(Some(&observed), &result);
        assert_eq!(body.text, "Diagnostic retained");
        assert_eq!(body.copy_text(), "Diagnostic retained");
        assert!(body.notices.is_empty());
    }
}

#[test]
fn partial_failures() {
    let result = json!({"available":false,"error":"Provider returned HTTP 402","sailry_result":{"version":1,"diagnostics":[{"text":"Provider returned HTTP 402","error":true}]}});
    assert!(!is_error(None, Some(&result)));
    assert_eq!(
        failure(None, Some(&result)).as_deref(),
        Some("Provider returned HTTP 402")
    );
    assert!(tool_preview(None, &result).copy_text().contains("HTTP 402"));
    assert!(failure(None, Some(&json!({"available":true,"error":null}))).is_none());
    assert!(failure(None, Some(&json!({"error":"literal file content"}))).is_none());
}

#[test]
fn mcp_errors_preserve_text() {
    let first = "Input failed\n[detail](literal) 中文";
    let second = "  Original diagnostic  ";
    let expected = format!("{first}\n{second}");
    for value in [
        json!({"error":expected}),
        json!({"isError":true,"content":[
            {"type":"text","text":first},
            {"type":"image","text":"Not diagnostic text","mimeType":"image/png"},
            {"type":"text","text":"  "},
            {"type":"text","text":second}
        ],"structuredContent":{"code":"input_failed","details":{"original":true}},"_meta":{"source":"native"}}),
    ] {
        let original = value.clone();
        assert!(is_error(Some(&FAILED), Some(&value)));
        assert_eq!(
            failure(Some(&FAILED), Some(&value)).as_deref(),
            Some(expected.as_str())
        );
        let body = tool_preview(Some(&FAILED), &value);
        assert_eq!(body.text, expected);
        assert_eq!(body.copy_text(), expected);
        assert!(body.notices.is_empty());
        assert_eq!(value, original);
    }
}

#[test]
fn mcp_error_fields_preserve_user_data() {
    for result in [
        json!({"isError":false,"structuredContent":{"isError":true,"error":"domain row"}}),
        json!({"output":{"isError":true,"error":"domain row"}}),
        json!({"isError":true,"error":"domain row"}),
        json!({"isError":"true","content":[{"type":"text","text":"Literal content"}]}),
        json!({"error":Fault::new(ErrorCode::Unavailable,"domain row")}),
    ] {
        let original = result.clone();
        assert!(!is_error(None, Some(&result)));
        assert!(fault(None, Some(&result)).is_none());
        assert!(failure(None, Some(&result)).is_none());
        assert_eq!(
            tool_preview(None, &result).text,
            preview(None, &result).text
        );
        assert!(tool_preview(None, &result).notices.is_empty());
        assert_eq!(
            serde_json::from_str::<Value>(&raw(&result)).unwrap(),
            original
        );
        assert_eq!(result, original);
    }
}

#[test]
fn mcp_errors_preserve_missing_diagnostics() {
    for result in [
        json!({"isError":true,"content":[]}),
        json!({"isError":true,"content":[{"type":"image","text":"Not diagnostic text"}]}),
        json!({"isError":true,"content":[{"type":"text","text":false}]}),
        json!({"output":{"isError":true,"error":"domain row"}}),
    ] {
        assert!(is_error(Some(&FAILED), Some(&result)));
        assert!(failure(Some(&FAILED), Some(&result)).is_none());
        assert_eq!(
            tool_preview(Some(&FAILED), &result).notices,
            ["tool_failed"]
        );
    }
}

#[test]
fn invalid_markers_preserve_user_data() {
    for marker in [
        json!({"version":2,"diagnostics":[{"text":"user data","error":true}]}),
        json!({"version":1,"diagnostics":[{"text":"user data","error":"yes"}]}),
    ] {
        let result = json!({"error":"literal field","sailry_result":marker});
        assert!(!is_error(None, Some(&result)));
        assert!(failure(None, Some(&result)).is_none());
        assert!(ResultDisplay::from_value(&result).is_none());
        assert!(tool_preview(None, &result).text.is_empty());
        let inspected = raw(&result);
        assert!(inspected.contains("literal field"));
        assert!(inspected.contains("user data"));
        assert_eq!(serde_json::from_str::<Value>(&inspected).unwrap(), result);
    }
}

#[test]
fn preserves_faults() {
    let message = "file already exists: assets/image.png\nChoose another output path";
    let value = json!({"error":Fault::new(ErrorCode::RevisionConflict, message)});
    let observed = Failure {
        code: Some(ErrorCode::RevisionConflict),
    };
    let rendered = tool_preview(Some(&observed), &value);
    assert_eq!(rendered.text, message);
    assert_eq!(rendered.copy_text(), message);
    assert!(rendered.notices.is_empty());
}

#[test]
fn shows_content_and_limits() {
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
    }))
    .unwrap();
    let projection = file_projection("read_file");
    let arguments = json!({"path":"source.rs"});
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
    let search = serde_json::to_value(Output::SearchResults(SearchResults {
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
    let arguments = json!({"query":"matched text"});
    assert_eq!(
        projection
            .preview
            .as_ref()
            .unwrap()
            .render(&arguments, &search),
        "资料.txt:3  matched text"
    );
    assert_eq!(
        projection.notices(&arguments, &search, "en"),
        ["Partial results"]
    );
    let body = preview(None, &search);
    assert!(body.text.is_empty());
    assert_eq!(
        serde_json::from_str::<Value>(&raw(&search)).unwrap(),
        search
    );
    assert!(body.notices.is_empty());
}

#[test]
fn separates_diagnostics() {
    let result = json!({"error": Fault::new(ErrorCode::PermissionDenied, "protected diagnostic")});
    let observed = Failure {
        code: Some(ErrorCode::PermissionDenied),
    };
    assert_eq!(
        fault(Some(&observed), Some(&result)).unwrap().code,
        ErrorCode::PermissionDenied
    );
    let body = preview(Some(&observed), &result);
    assert!(body.text.is_empty());
    assert_eq!(body.notices, ["tool_denied"]);
    assert!(fault(None, Some(&result)).is_none());
    assert!(preview(None, &result).notices.is_empty());
    assert!(fault(Some(&observed), Some(&json!({"error":"user data"}))).is_none());
    let unknown = Failure {
        code: Some(ErrorCode::OutcomeUnknown),
    };
    assert!(!is_error(Some(&unknown), Some(&result)));
    assert_eq!(
        preview(Some(&unknown), &result).notices,
        ["tool_interrupted"]
    );
    let body = preview(None, &json!({"text":"output","count":2}));
    assert!(body.text.is_empty());
    assert_eq!(
        serde_json::from_str::<Value>(&raw(&json!({"text":"output","count":2}))).unwrap(),
        json!({"text":"output","count":2})
    );
    assert!(preview(None, &json!(null)).notices.is_empty());
}

#[test]
fn shows_file_write_body() {
    let result = serde_json::to_value(Output::FileWritten(sailry_protocol::FileWritten {
        path: "资料.txt".into(),
        revision: "opaque-revision".into(),
        size: 3,
    }))
    .unwrap();
    let projection = file_projection("write_file");
    let arguments = json!({"path":"资料.txt","text":"文","expected_revision":null});
    let declared_body = projection.body.as_ref().unwrap();
    assert_eq!(
        declared_body.text.read(&arguments, &result),
        Some(&json!("文"))
    );
    assert_eq!(
        declared_body
            .path
            .as_ref()
            .unwrap()
            .read(&arguments, &result),
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

#[test]
fn browser_failure_categories() {
    for (code, message, expected) in [
        (
            ErrorCode::NotFound,
            "browser tab is empty; navigate to a URL before reading or interacting",
            "tool_not_found",
        ),
        (
            ErrorCode::Unavailable,
            "browser page is unavailable",
            "tool_unavailable",
        ),
        (ErrorCode::Unavailable, "unavailable", "tool_unavailable"),
        (ErrorCode::OutcomeUnknown, "unknown", "tool_interrupted"),
    ] {
        let observed = Failure { code: Some(code) };
        assert_eq!(
            preview(Some(&observed), &json!({"error":Fault::new(code,message)})).notices,
            [expected]
        );
    }
}

#[test]
fn preserves_sql_diagnostics() {
    let message = "Database query failed: syntax error near quoted_table";
    let result = json!({"error":Fault::invalid(message)});
    let observed = Failure {
        code: Some(ErrorCode::InvalidRequest),
    };
    let body = preview(Some(&observed), &result);
    assert!(body.text.is_empty());
    assert_eq!(body.notices, ["tool_invalid"]);
    let body = tool_preview(Some(&observed), &result);
    assert!(body.notices.is_empty());
    assert_eq!(body.text, message);
    assert_eq!(body.copy_text(), message);
}

#[test]
fn undeclared_values_keep_business_fields() {
    for result in [
        json!({"response":{"error":"input failed"}}),
        json!({"error":"literal"}),
    ] {
        assert!(failure(None, Some(&result)).is_none());
        assert!(!is_error(None, Some(&result)));
        assert_eq!(
            tool_preview(None, &result).text,
            preview(None, &result).text
        );
    }
}

#[test]
fn observed_failures_keep_their_messages() {
    let message = "Before-tool hook failed\nLiteral [diagnostic] 中文 🙂";
    for result in [
        json!({"error":message}),
        json!({"isError":true,"error":message}),
        json!({"isError":false,"error":message}),
    ] {
        assert!(is_error(Some(&FAILED), Some(&result)));
        assert_eq!(
            failure(Some(&FAILED), Some(&result)).as_deref(),
            Some(message)
        );
        let body = tool_preview(Some(&FAILED), &result);
        assert_eq!(body.text, message);
        assert_eq!(body.copy_text(), message);
        assert!(body.notices.is_empty());
        assert!(!is_error(None, Some(&result)));
        assert!(failure(None, Some(&result)).is_none());
    }
}
