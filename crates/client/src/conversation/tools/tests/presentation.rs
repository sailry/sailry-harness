use super::*;
use sailry_protocol::{ErrorCode, Fault, tool::Presentation};

fn returned(presentation: Presentation, result: Value) -> Page {
    let mut page = page();
    if let Part::ToolCall {
        presentation: mode, ..
    } = &mut page.entries[0].parts[0]
    {
        *mode = presentation;
    }
    let mut response = entry(2, page.runs[0].turn, "", true);
    if let Part::ToolResult { result: value, .. } = &mut response.parts[0] {
        *value = result;
    }
    page.entries.push(response);
    page
}

fn declare(page: &mut Page, value: Value) {
    if let Part::ToolCall { display, .. } = &mut page.entries[0].parts[0] {
        *display = Some(serde_json::from_value(value).unwrap());
    }
}

#[test]
fn summaries_keep_original_data_without_details() {
    let result =
        json!({"node":[119,14,79],"id":"business-id","revision":9,"error":"business value"});
    let mut page = returned(Presentation::default(), result.clone());
    assert!(!collect(&page)[0].details(&page));
    assert!(!collect(&page)[0].hidden(&page));
    assert!(!collect(&page)[0].diagnostics(&page));
    assert_eq!(collect(&page)[0].result(&page), Some(&result));
    declare(
        &mut page,
        json!({"label":"External read","raw_result":true}),
    );
    let calls = collect(&page);
    assert!(calls[0].raw_result(&page));
    assert!(!calls[0].details(&page));
    assert_eq!(calls[0].result(&page), Some(&result));
}

#[test]
fn images_follow_the_declared_mode() {
    let mut page = returned(Presentation::Summary, json!({"data":"produced image"}));
    if let Part::ToolResult { images, .. } = &mut page.entries[1].parts[0] {
        images.push(sailry_protocol::conversation::Image {
            entry: "event-2".into(),
            part: 0,
            index: 0,
            attachment: sailry_protocol::attachment::Attachment {
                id: sailry_protocol::AttachmentId::new(),
                spec: sailry_protocol::attachment::Spec {
                    worktree: sailry_protocol::WorktreeId::new(),
                    name: "image.png".into(),
                    media_type: "image/png".into(),
                    size: 1,
                    revision: "a".repeat(64),
                },
            },
        });
    }
    assert!(!collect(&page)[0].details(&page));
    if let Part::ToolCall { presentation, .. } = &mut page.entries[0].parts[0] {
        *presentation = Presentation::Details;
    }
    declare(
        &mut page,
        json!({"label":"Capture","output":{"preview":{"parts":[{"value":{"source":"result","path":"/absent"}}]}}}),
    );
    assert!(collect(&page)[0].details(&page));
}

#[test]
fn declared_empty_output_suppresses_fallback() {
    let mut page = returned(
        Presentation::Details,
        json!({"node":[119,14,79],"title":"real"}),
    );
    declare(
        &mut page,
        json!({"label":"Manage","output":{"preview":{"parts":[{
            "value":{"source":"result","path":"/title"},
            "when":{"all":[{"value":{"source":"arguments","path":"/action"},"equals":"update"}]}
        }]}}}),
    );
    assert!(!collect(&page)[0].details(&page));
    if let Part::ToolCall { arguments, .. } = &mut page.entries[0].parts[0] {
        *arguments = json!({"action":"update"});
    }
    assert!(collect(&page)[0].details(&page));
    let original = page.clone();
    let restored: Page = serde_json::from_value(serde_json::to_value(&page).unwrap()).unwrap();
    assert!(collect(&restored)[0].details(&restored));
    assert_eq!(restored, original);
}

#[test]
fn hidden_success_keeps_failures_and_interactions() {
    let mut page = returned(Presentation::Hidden, json!({"title":"Renamed"}));
    assert!(collect(&page)[0].hidden(&page));
    assert!(!collect(&page)[0].details(&page));
    for code in [
        ErrorCode::RevisionConflict,
        ErrorCode::Cancelled,
        ErrorCode::OutcomeUnknown,
    ] {
        if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
            *result = json!({"error":Fault::new(code,"Original diagnostic")});
        }
        let call = &collect(&page)[0];
        assert!(!call.hidden(&page));
        assert!(call.details(&page));
    }
    if let Part::ToolCall { name, .. } = &mut page.entries[0].parts[0] {
        *name = "ask_user".into();
    }
    if let Part::ToolResult { name, result, .. } = &mut page.entries[1].parts[0] {
        *name = "ask_user".into();
        *result = json!({"answer":"Accepted"});
    }
    assert!(!collect(&page)[0].hidden(&page));
}

#[test]
fn failure_facts_require_the_exact_response() {
    let mut page = returned(Presentation::Hidden, json!({"error":"Actual tool failure"}));
    for (index, id, name, attaches) in [
        (1, "call", "read_file", false),
        (0, "other", "read_file", false),
        (0, "call", "other", false),
        (0, "call", "read_file", true),
    ] {
        page.entries[1].parts.truncate(1);
        page.entries[1]
            .parts
            .push(Part::Resource(json!({"type":"tool_failed",
            "index":index,"id":id,"name":name,"failure":{}})));
        let calls = collect(&page);
        assert_eq!(calls[0].failure.is_some(), attaches);
        assert_eq!(calls[0].details(&page), attaches);
        assert_eq!(calls[0].hidden(&page), !attaches);
        assert_eq!(
            calls[0].result(&page),
            Some(&json!({"error":"Actual tool failure"}))
        );
    }
}

#[test]
fn json_formatting_is_deferred_until_inspection() {
    let body = "Literal 内容 🙂\n".repeat(16_000);
    let result = json!({"data":{"id":"business-id","node":[1,2,3],"body":body}});
    let mut page = returned(Presentation::Details, result.clone());
    declare(
        &mut page,
        json!({"label":"Invoke","output":{"preview":{"parts":[{
            "value":{"source":"result","path":"/data"},"format":"json",
            "when":{"all":[{"value":{"source":"arguments","path":"/action"},"equals":"invoke"}]}
        }]}}}),
    );
    let inactive = collect(&page);
    assert!(!inactive[0].details(&page));
    assert!(
        serde_json::to_value(&inactive[0]).unwrap()["resolved"]["output"]
            .get("preview_deferred")
            .is_none()
    );
    if let Part::ToolCall { arguments, .. } = &mut page.entries[0].parts[0] {
        *arguments = json!({"action":"invoke"});
    }
    let calls = collect(&page);
    assert!(calls[0].details(&page));
    let output = calls[0].resolved.as_ref().unwrap().output.as_ref().unwrap();
    assert!(output.preview.is_empty());
    assert!(output.preview_deferred);
    let serialized = serde_json::to_value(&calls[0]).unwrap();
    assert_eq!(serialized["resolved"]["output"]["preview_deferred"], true);
    assert!(!serialized.to_string().contains("business-id"));
    assert_eq!(calls[0].result(&page), Some(&result));
}

#[test]
fn writes_without_columns_keep_their_receipt() {
    let manifest: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/databases/plugin.json"
    )))
    .unwrap();
    let tool = manifest["extensions"]["dev.sailry.platform"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "database_execute")
        .unwrap();
    let result = serde_json::to_value(sailry_protocol::Output::DatabaseOutcome(
        sailry_protocol::database::Outcome::Query(sailry_protocol::database::ResultSet {
            affected_rows: 2,
            ..Default::default()
        }),
    ))
    .unwrap();
    let mut page = returned(Presentation::Details, result.clone());
    declare(&mut page, tool["display"].clone());
    let calls = collect(&page);
    assert!(calls[0].details(&page));
    let output = calls[0].resolved.as_ref().unwrap().output.as_ref().unwrap();
    assert!(output.table.is_none());
    assert_eq!(output.preview, "affected_rows: 2");
    assert_eq!(calls[0].result(&page), Some(&result));
}

#[test]
fn receipt_names_do_not_require_argument_titles() {
    let manifest: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/memory/plugin.json"
    )))
    .unwrap();
    let tools = manifest["extensions"]["dev.sailry.platform"]["tools"]
        .as_array()
        .unwrap();
    let arguments = json!({"id":"11111111-2222-4333-8444-555555555555","expected_revision":1});
    for (name, arguments, result, expected) in [
        (
            "forget_memory",
            arguments.clone(),
            json!({"receipt":{"title":"Accepted preference 中文 🙂"}}),
            Some("Accepted preference 中文 🙂"),
        ),
        (
            "forget_memory",
            arguments.clone(),
            json!({"status":"pending","saved":false}),
            None,
        ),
        (
            "forget_memory",
            arguments,
            json!({"receipt":{"title":[119,14,79]}}),
            None,
        ),
        (
            "save_memory",
            json!({"title":"Argument preference"}),
            json!({"receipt":{"title":" \n "}}),
            Some("Argument preference"),
        ),
        (
            "save_memory",
            json!({"title":"Argument preference"}),
            json!({"receipt":{"title":{"id":"internal"}}}),
            Some("Argument preference"),
        ),
    ] {
        let declaration = tools.iter().find(|tool| tool["name"] == name).unwrap();
        let mut page = returned(Presentation::Details, result.clone());
        if let Part::ToolCall {
            arguments: input, ..
        } = &mut page.entries[0].parts[0]
        {
            *input = arguments.clone();
        }
        declare(&mut page, declaration["display"].clone());
        let calls = collect(&page);
        let summary = calls[0].resolved.as_ref().unwrap().input.summary.as_ref();
        assert_eq!(
            summary.map(|summary| summary.text.as_str()),
            expected,
            "{name}"
        );
        assert_eq!(calls[0].arguments(&page), Some(&arguments));
        assert_eq!(calls[0].result(&page), Some(&result));
    }
}
