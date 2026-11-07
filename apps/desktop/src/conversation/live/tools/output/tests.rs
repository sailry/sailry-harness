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
fn diagnostics_and_content() {
    for (scenario, check) in [
        (
            "declared_diagnostics_keep_their_tone_and_text",
            (|_scenario: &str| {
                let diagnostic =
                    "Verification did not confirm the requested state\n[detail](literal) 中文";
                let result = json!({"response":{"unrelated":"opaque output"},"inline_data":["image bytes"],
        "sailry_result":{"version":1,"diagnostics":[{"text":diagnostic,"error":false}],
            "status":{"label":"Condition not met"}}});
                assert!(
                    !is_error(None, Some(&result)),
                    "declared diagnostics keep their tone and text"
                );
                assert_eq!(
                    failure(None, Some(&result)).as_deref(),
                    Some(diagnostic),
                    "declared diagnostics keep their tone and text"
                );
                assert_eq!(
                    ResultDisplay::from_value(&result)
                        .unwrap()
                        .status
                        .unwrap()
                        .label,
                    "Condition not met",
                    "declared diagnostics keep their tone and text"
                );
                let body = tool_preview(None, &result);
                assert_eq!(
                    body.text, diagnostic,
                    "declared diagnostics keep their tone and text"
                );
                assert_eq!(
                    body.copy_text(),
                    diagnostic,
                    "declared diagnostics keep their tone and text"
                );
                assert!(
                    body.notices.is_empty(),
                    "declared diagnostics keep their tone and text"
                );
            }) as fn(&str),
        ),
        (
            "declared_status_cannot_hide_errors",
            (|_scenario: &str| {
                let marker = json!({"version":1,"diagnostics":[{"text":"Condition diagnostic","error":false}],
        "status":{"label":"Condition not met"}});
                for mut result in [
                    json!({"isError":true}),
                    json!({"output":{"isError":true}}),
                    json!({"error":Fault::new(ErrorCode::Unavailable,"Worker disconnected")}),
                ] {
                    result["sailry_result"] = marker.clone();
                    assert!(
                        is_error(Some(&FAILED), Some(&result)),
                        "declared status cannot hide errors"
                    );
                    let diagnostic = if fault(Some(&FAILED), Some(&result)).is_some() {
                        "Worker disconnected"
                    } else {
                        "Condition diagnostic"
                    };
                    let body = tool_preview(Some(&FAILED), &result);
                    assert_eq!(body.text, diagnostic, "declared status cannot hide errors");
                    assert_eq!(
                        body.copy_text(),
                        diagnostic,
                        "declared status cannot hide errors"
                    );
                    assert!(
                        body.notices.is_empty(),
                        "declared status cannot hide errors"
                    );
                }
                let result = json!({"sailry_result":{"version":1,"diagnostics":[
                {"text":"Condition diagnostic","error":false},{"text":"Worker disconnected","error":true}],
        "status":{"label":"Condition not met"}}});
                assert!(
                    !is_error(None, Some(&result)),
                    "declared status cannot hide errors"
                );
                assert!(
                    is_error(Some(&FAILED), Some(&result)),
                    "declared status cannot hide errors"
                );
                assert_eq!(
                    tool_preview(None, &result).copy_text(),
                    "Condition diagnostic\nWorker disconnected",
                    "declared status cannot hide errors"
                );
            }),
        ),
        (
            "cancellation_and_uncertainty_are_not_execution_errors",
            (|_scenario: &str| {
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
                        matches!(code, ErrorCode::Unavailable | ErrorCode::InvalidRequest),
                        "cancellation and uncertainty are not execution errors"
                    );
                    assert_eq!(
                        failure(Some(&observed), Some(&result)).as_deref(),
                        Some("Diagnostic retained"),
                        "cancellation and uncertainty are not execution errors"
                    );
                    let body = tool_preview(Some(&observed), &result);
                    assert_eq!(
                        body.text, "Diagnostic retained",
                        "cancellation and uncertainty are not execution errors"
                    );
                    assert_eq!(
                        body.copy_text(),
                        "Diagnostic retained",
                        "cancellation and uncertainty are not execution errors"
                    );
                    assert!(
                        body.notices.is_empty(),
                        "cancellation and uncertainty are not execution errors"
                    );
                }
            }),
        ),
        (
            "partial_failures",
            (|_scenario: &str| {
                let result = json!({"available":false,"error":"Provider returned HTTP 402","sailry_result":{"version":1,"diagnostics":[{"text":"Provider returned HTTP 402","error":true}]}});
                assert!(!is_error(None, Some(&result)), "partial failures");
                assert_eq!(
                    failure(None, Some(&result)).as_deref(),
                    Some("Provider returned HTTP 402"),
                    "partial failures"
                );
                assert!(
                    tool_preview(None, &result).copy_text().contains("HTTP 402"),
                    "partial failures"
                );
                assert!(
                    failure(None, Some(&json!({"available":true,"error":null}))).is_none(),
                    "partial failures"
                );
                assert!(
                    failure(None, Some(&json!({"error":"literal file content"}))).is_none(),
                    "partial failures"
                );
            }),
        ),
        (
            "mcp_errors_preserve_text",
            (|_scenario: &str| {
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
                    assert!(
                        is_error(Some(&FAILED), Some(&value)),
                        "mcp errors preserve text"
                    );
                    assert_eq!(
                        failure(Some(&FAILED), Some(&value)).as_deref(),
                        Some(expected.as_str()),
                        "mcp errors preserve text"
                    );
                    let body = tool_preview(Some(&FAILED), &value);
                    assert_eq!(body.text, expected, "mcp errors preserve text");
                    assert_eq!(body.copy_text(), expected, "mcp errors preserve text");
                    assert!(body.notices.is_empty(), "mcp errors preserve text");
                    assert_eq!(value, original, "mcp errors preserve text");
                }
            }),
        ),
        (
            "mcp_error_fields_preserve_user_data",
            (|_scenario: &str| {
                for result in [
                    json!({"isError":false,"structuredContent":{"isError":true,"error":"domain row"}}),
                    json!({"output":{"isError":true,"error":"domain row"}}),
                    json!({"isError":true,"error":"domain row"}),
                    json!({"isError":"true","content":[{"type":"text","text":"Literal content"}]}),
                    json!({"error":Fault::new(ErrorCode::Unavailable,"domain row")}),
                ] {
                    let original = result.clone();
                    assert!(
                        !is_error(None, Some(&result)),
                        "mcp error fields preserve user data"
                    );
                    assert!(
                        fault(None, Some(&result)).is_none(),
                        "mcp error fields preserve user data"
                    );
                    assert!(
                        failure(None, Some(&result)).is_none(),
                        "mcp error fields preserve user data"
                    );
                    assert_eq!(
                        tool_preview(None, &result).text,
                        preview(None, &result).text,
                        "mcp error fields preserve user data"
                    );
                    assert!(
                        tool_preview(None, &result).notices.is_empty(),
                        "mcp error fields preserve user data"
                    );
                    assert_eq!(
                        serde_json::from_str::<Value>(&raw(&result)).unwrap(),
                        original,
                        "mcp error fields preserve user data"
                    );
                    assert_eq!(result, original, "mcp error fields preserve user data");
                }
            }),
        ),
        (
            "mcp_errors_preserve_missing_diagnostics",
            (|_scenario: &str| {
                for result in [
                    json!({"isError":true,"content":[]}),
                    json!({"isError":true,"content":[{"type":"image","text":"Not diagnostic text"}]}),
                    json!({"isError":true,"content":[{"type":"text","text":false}]}),
                    json!({"output":{"isError":true,"error":"domain row"}}),
                ] {
                    assert!(
                        is_error(Some(&FAILED), Some(&result)),
                        "mcp errors preserve missing diagnostics"
                    );
                    assert!(
                        failure(Some(&FAILED), Some(&result)).is_none(),
                        "mcp errors preserve missing diagnostics"
                    );
                    assert_eq!(
                        tool_preview(Some(&FAILED), &result).notices,
                        ["tool_failed"],
                        "mcp errors preserve missing diagnostics"
                    );
                }
            }),
        ),
        (
            "invalid_markers_preserve_user_data",
            (|_scenario: &str| {
                for marker in [
                    json!({"version":2,"diagnostics":[{"text":"user data","error":true}]}),
                    json!({"version":1,"diagnostics":[{"text":"user data","error":"yes"}]}),
                ] {
                    let result = json!({"error":"literal field","sailry_result":marker});
                    assert!(
                        !is_error(None, Some(&result)),
                        "invalid markers preserve user data"
                    );
                    assert!(
                        failure(None, Some(&result)).is_none(),
                        "invalid markers preserve user data"
                    );
                    assert!(
                        ResultDisplay::from_value(&result).is_none(),
                        "invalid markers preserve user data"
                    );
                    assert!(
                        tool_preview(None, &result).text.is_empty(),
                        "invalid markers preserve user data"
                    );
                    let inspected = raw(&result);
                    assert!(
                        inspected.contains("literal field"),
                        "invalid markers preserve user data"
                    );
                    assert!(
                        inspected.contains("user data"),
                        "invalid markers preserve user data"
                    );
                    assert_eq!(
                        serde_json::from_str::<Value>(&inspected).unwrap(),
                        result,
                        "invalid markers preserve user data"
                    );
                }
            }),
        ),
        (
            "preserves_faults",
            (|_scenario: &str| {
                let message = "file already exists: assets/image.png\nChoose another output path";
                let value = json!({"error":Fault::new(ErrorCode::RevisionConflict, message)});
                let observed = Failure {
                    code: Some(ErrorCode::RevisionConflict),
                };
                let rendered = tool_preview(Some(&observed), &value);
                assert_eq!(rendered.text, message, "preserves faults");
                assert_eq!(rendered.copy_text(), message, "preserves faults");
                assert!(rendered.notices.is_empty(), "preserves faults");
            }),
        ),
        (
            "shows_content_and_limits",
            (|_scenario: &str| {
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
                let arguments = json!({"path":"source.rs"});
                let text = projection
                    .preview
                    .as_ref()
                    .unwrap()
                    .render(&arguments, &result);
                assert_eq!(
                    text, "1: 中文 🙂\n2: [content](file)\n3: ```rust\n4: let value = 1;\n5: ```",
                    "shows content and limits"
                );
                assert_eq!(
                    projection.notices(&arguments, &result, "en"),
                    ["Partial results"],
                    "shows content and limits"
                );
                let body = preview(None, &result);
                assert!(body.text.is_empty(), "shows content and limits");
                assert_eq!(
                    serde_json::from_str::<Value>(&raw(&result)).unwrap(),
                    result,
                    "shows content and limits"
                );
                assert!(body.notices.is_empty(), "shows content and limits");
                assert!(
                    literal(&text).starts_with("````\n"),
                    "shows content and limits"
                );
                assert!(
                    literal(&text).ends_with("\n````"),
                    "shows content and limits"
                );
                let mut final_page = result.clone();
                final_page["data"]["next_offset"] = json!(null);
                assert!(
                    projection.notices(&arguments, &final_page, "en").is_empty(),
                    "shows content and limits"
                );
                final_page["data"]["offset"] = json!(1);
                assert_eq!(
                    projection.notices(&arguments, &final_page, "en"),
                    ["Partial results"],
                    "shows content and limits"
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
                    "资料.txt:3  matched text",
                    "shows content and limits"
                );
                assert_eq!(
                    projection.notices(&arguments, &search, "en"),
                    ["Partial results"],
                    "shows content and limits"
                );
                let body = preview(None, &search);
                assert!(body.text.is_empty(), "shows content and limits");
                assert_eq!(
                    serde_json::from_str::<Value>(&raw(&search)).unwrap(),
                    search,
                    "shows content and limits"
                );
                assert!(body.notices.is_empty(), "shows content and limits");
            }),
        ),
        (
            "separates_diagnostics",
            (|_scenario: &str| {
                let result = json!({"error": Fault::new(ErrorCode::PermissionDenied, "protected diagnostic")});
                let observed = Failure {
                    code: Some(ErrorCode::PermissionDenied),
                };
                assert_eq!(
                    fault(Some(&observed), Some(&result)).unwrap().code,
                    ErrorCode::PermissionDenied,
                    "separates diagnostics"
                );
                let body = preview(Some(&observed), &result);
                assert!(body.text.is_empty(), "separates diagnostics");
                assert_eq!(body.notices, ["tool_denied"], "separates diagnostics");
                assert!(
                    fault(None, Some(&result)).is_none(),
                    "separates diagnostics"
                );
                assert!(
                    preview(None, &result).notices.is_empty(),
                    "separates diagnostics"
                );
                assert!(
                    fault(Some(&observed), Some(&json!({"error":"user data"}))).is_none(),
                    "separates diagnostics"
                );
                let unknown = Failure {
                    code: Some(ErrorCode::OutcomeUnknown),
                };
                assert!(
                    !is_error(Some(&unknown), Some(&result)),
                    "separates diagnostics"
                );
                assert_eq!(
                    preview(Some(&unknown), &result).notices,
                    ["tool_interrupted"],
                    "separates diagnostics"
                );
                let body = preview(None, &json!({"text":"output","count":2}));
                assert!(body.text.is_empty(), "separates diagnostics");
                assert_eq!(
                    serde_json::from_str::<Value>(&raw(&json!({"text":"output","count":2})))
                        .unwrap(),
                    json!({"text":"output","count":2}),
                    "separates diagnostics"
                );
                assert!(
                    preview(None, &json!(null)).notices.is_empty(),
                    "separates diagnostics"
                );
            }),
        ),
        (
            "shows_file_write_body",
            (|_scenario: &str| {
                let result =
                    serde_json::to_value(Output::FileWritten(sailry_protocol::FileWritten {
                        path: "资料.txt".into(),
                        revision: "opaque-revision".into(),
                        size: 3,
                        excerpt: None,
                    }))
                    .unwrap();
                let projection = file_projection("write_file");
                let arguments = json!({"path":"资料.txt","text":"文","expected_revision":null});
                let declared_body = projection.body.as_ref().unwrap();
                assert_eq!(
                    declared_body.text.read(&arguments, &result),
                    Some(&json!("文")),
                    "shows file write body"
                );
                assert_eq!(
                    declared_body
                        .path
                        .as_ref()
                        .unwrap()
                        .read(&arguments, &result),
                    Some(&json!("资料.txt")),
                    "shows file write body"
                );
                assert!(
                    projection.notices(&arguments, &result, "en").is_empty(),
                    "shows file write body"
                );
                let body = preview(None, &result);
                assert!(body.text.is_empty(), "shows file write body");
                assert_eq!(
                    serde_json::from_str::<Value>(&raw(&result)).unwrap(),
                    result,
                    "shows file write body"
                );
                assert!(body.notices.is_empty(), "shows file write body");
            }),
        ),
        (
            "browser_failure_categories",
            (|_scenario: &str| {
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
                        preview(Some(&observed), &json!({"error":Fault::new(code,message)}))
                            .notices,
                        [expected],
                        "browser failure categories"
                    );
                }
            }),
        ),
        (
            "preserves_sql_diagnostics",
            (|_scenario: &str| {
                let message = "Database query failed: syntax error near quoted_table";
                let result = json!({"error":Fault::invalid(message)});
                let observed = Failure {
                    code: Some(ErrorCode::InvalidRequest),
                };
                let body = preview(Some(&observed), &result);
                assert!(body.text.is_empty(), "preserves sql diagnostics");
                assert_eq!(body.notices, ["tool_invalid"], "preserves sql diagnostics");
                let body = tool_preview(Some(&observed), &result);
                assert!(body.notices.is_empty(), "preserves sql diagnostics");
                assert_eq!(body.text, message, "preserves sql diagnostics");
                assert_eq!(body.copy_text(), message, "preserves sql diagnostics");
            }),
        ),
        (
            "undeclared_values_keep_business_fields",
            (|_scenario: &str| {
                for result in [
                    json!({"response":{"error":"input failed"}}),
                    json!({"error":"literal"}),
                ] {
                    assert!(
                        failure(None, Some(&result)).is_none(),
                        "undeclared values keep business fields"
                    );
                    assert!(
                        !is_error(None, Some(&result)),
                        "undeclared values keep business fields"
                    );
                    assert_eq!(
                        tool_preview(None, &result).text,
                        preview(None, &result).text,
                        "undeclared values keep business fields"
                    );
                }
            }),
        ),
        (
            "observed_failures_keep_their_messages",
            (|_scenario: &str| {
                let message = "Before-tool hook failed\nLiteral [diagnostic] 中文 🙂";
                for result in [
                    json!({"error":message}),
                    json!({"isError":true,"error":message}),
                    json!({"isError":false,"error":message}),
                ] {
                    assert!(
                        is_error(Some(&FAILED), Some(&result)),
                        "observed failures keep their messages"
                    );
                    assert_eq!(
                        failure(Some(&FAILED), Some(&result)).as_deref(),
                        Some(message),
                        "observed failures keep their messages"
                    );
                    let body = tool_preview(Some(&FAILED), &result);
                    assert_eq!(body.text, message, "observed failures keep their messages");
                    assert_eq!(
                        body.copy_text(),
                        message,
                        "observed failures keep their messages"
                    );
                    assert!(
                        body.notices.is_empty(),
                        "observed failures keep their messages"
                    );
                    assert!(
                        !is_error(None, Some(&result)),
                        "observed failures keep their messages"
                    );
                    assert!(
                        failure(None, Some(&result)).is_none(),
                        "observed failures keep their messages"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
