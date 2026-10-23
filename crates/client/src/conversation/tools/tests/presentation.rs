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

fn mark_failure(page: &mut Page, code: Option<ErrorCode>) {
    page.entries[1].parts.truncate(1);
    page.entries[1]
        .parts
        .push(Part::Resource(json!({"type":"tool_failed",
        "index":0,"id":"call","name":"read_file","failure":{"code":code}})));
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
fn opaque_returns_keep_activity_only() {
    for result in [
        Value::Null,
        json!({"pending":true,"status":"running"}),
        json!([null, "opaque"]),
    ] {
        let page = returned(Presentation::Details, result.clone());
        let calls = collect(&page);
        assert!(!calls[0].details(&page));
        assert!(calls[0].output(&page).is_none());
        assert_eq!(calls[0].result(&page), Some(&result));
    }
    for (result, preview) in [
        (json!("Direct output"), "Direct output"),
        (json!(["First", "Second"]), "First\nSecond"),
        (json!(0), "0"),
        (json!(false), "false"),
    ] {
        let page = returned(Presentation::Details, result.clone());
        let calls = collect(&page);
        assert!(calls[0].details(&page));
        assert_eq!(calls[0].output(&page).unwrap().preview, preview);
        assert_eq!(calls[0].result(&page), Some(&result));
    }
}

#[test]
fn images_preserve_inspection() {
    let mut page = returned(Presentation::Summary, Value::Null);
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
    for mode in [
        Presentation::Summary,
        Presentation::Hidden,
        Presentation::Details,
        Presentation::Content,
    ] {
        if let Part::ToolCall { presentation, .. } = &mut page.entries[0].parts[0] {
            *presentation = mode;
        }
        let calls = collect(&page);
        assert!(calls[0].details(&page));
        assert!(!calls[0].hidden(&page));
        assert_eq!(calls[0].result(&page), Some(&Value::Null));
        assert_eq!(calls[0].images(&page).len(), 1);
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
fn diagnostics_suppress_success_previews() {
    for (result, declaration) in [
        (json!("Captured literal"), None),
        (
            json!({"isError":true,"error":"domain row"}),
            Some(json!({"label":"Execute","output":{"preview":{"parts":[
                {"text":"Completed"}
            ]}}})),
        ),
        (
            json!({"payload":{"value":"Original business data"}}),
            Some(json!({"label":"Execute","output":{"preview":{"parts":[{
                "value":{"source":"result","path":"/payload"},"format":"json"
            }]}}})),
        ),
    ] {
        let mut page = returned(Presentation::Details, result.clone());
        if let Some(declaration) = declaration {
            declare(&mut page, declaration);
        }
        assert!(collect(&page)[0].output(&page).is_some());
        mark_failure(&mut page, None);
        let calls = collect(&page);
        assert!(calls[0].details(&page));
        assert!(calls[0].output(&page).is_none());
        assert_eq!(calls[0].result(&page), Some(&result));
    }
    let mut page = returned(Presentation::Details, Value::Null);
    declare(
        &mut page,
        json!({"label":"Execute","output":{"preview":{"parts":[
            {"text":"Completed"}
        ]}}}),
    );
    page.entries.truncate(1);
    let calls = collect(&page);
    assert!(!calls[0].details(&page));
    assert!(calls[0].output(&page).is_none());
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
        mark_failure(&mut page, Some(code));
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
    let rendered = calls[0].output(&page).unwrap();
    assert_eq!(
        rendered.preview,
        serde_json::to_string_pretty(&result["data"]).unwrap()
    );
    assert!(!rendered.preview_deferred);
    let captured = calls[0].resolved.as_ref().unwrap().output.as_ref().unwrap();
    assert!(captured.preview.is_empty());
    assert!(captured.preview_deferred);
}

#[test]
fn batch_edits_keep_the_selected_preview() {
    let manifest: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/files/plugin.json"
    )))
    .unwrap();
    let declaration = manifest["extensions"]["dev.sailry.platform"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "edit_file")
        .unwrap();
    let arguments = json!({"path":"source.txt","edits":[
        {"old_text":"First","new_text":"Updated first"},
        {"old_text":"Second","new_text":"Updated second"}
    ],"expected_revision":"original"});
    let result = json!({"written":true});
    let mut page = returned(Presentation::Details, result.clone());
    if let Part::ToolCall {
        name,
        arguments: input,
        ..
    } = &mut page.entries[0].parts[0]
    {
        *name = "package_update".into();
        *input = arguments.clone();
    }
    if let Part::ToolResult { name, .. } = &mut page.entries[1].parts[0] {
        *name = "package_update".into();
    }
    declare(&mut page, declaration["display"].clone());
    let calls = collect(&page);
    assert!(calls[0].details(&page));
    let output = calls[0].output(&page).unwrap();
    assert!(output.body.is_none());
    assert_eq!(output.preview, "Updated first\nUpdated second");
    assert_eq!(output.notices[0].label, "Written");
    assert_eq!(calls[0].arguments(&page), Some(&arguments));
    assert_eq!(calls[0].result(&page), Some(&result));
}

#[test]
fn command_outcomes_require_declared_content() {
    use sailry_protocol::process::{Capture, Completion, Outcome};
    for outcome in [
        Outcome::Exited(7),
        Outcome::Signal(9),
        Outcome::TimedOut,
        Outcome::Cancelled,
        Outcome::Unknown("cleanup is unconfirmed".into()),
    ] {
        let mut result = serde_json::to_value(sailry_protocol::Output::CommandResult(Completion {
            outcome: outcome.clone(),
            stdout: Capture {
                text: "Original stdout".into(),
                ..Default::default()
            },
            stderr: Capture::default(),
            elapsed_ms: 1,
        }))
        .unwrap();
        result["sailry_content"] = json!({"version":1,"blocks":[
            {"kind":"text","path":"/data/stdout/text"}
        ]});
        let mut page = returned(Presentation::Content, result.clone());
        declare(
            &mut page,
            json!({"label":"Execute","input":{"summary":{
                "path":"/command","format":"command"
            }}}),
        );
        let calls = collect(&page);
        assert_eq!(calls[0].resolved.as_ref().unwrap().command, Some(outcome));
        assert!(calls[0].failure.is_none());
        assert!(!calls[0].diagnostics(&page));
        assert_eq!(calls[0].result(&page), Some(&result));

        declare(&mut page, json!({"label":"Read"}));
        assert!(
            collect(&page)[0]
                .resolved
                .as_ref()
                .unwrap()
                .command
                .is_none()
        );
        declare(
            &mut page,
            json!({"label":"Execute","input":{"summary":{
                "path":"/command","format":"command"
            }}}),
        );
        if let Part::ToolCall { presentation, .. } = &mut page.entries[0].parts[0] {
            *presentation = Presentation::Details;
        }
        assert!(
            collect(&page)[0]
                .resolved
                .as_ref()
                .unwrap()
                .command
                .is_none()
        );
        assert_eq!(collect(&page)[0].result(&page), Some(&result));
    }
    for mut result in [
        json!({"isError":true,"outcome":{"kind":"timed_out"},"data":{
            "stdout":{"text":"Business row","truncated":false,"invalid_utf8":false}
        }}),
        json!({"kind":"command_result","data":{
            "outcome":{"kind":"exited","data":7},
            "stdout":{"text":"Incomplete completion","truncated":false,"invalid_utf8":false}
        }}),
        json!({"kind":"plugin_result","data":{
            "outcome":{"kind":"signal","data":9},"elapsed_ms":1,
            "stdout":{"text":"Other protocol output","truncated":false,"invalid_utf8":false},
            "stderr":{"text":"","truncated":false,"invalid_utf8":false}
        }}),
    ] {
        result["sailry_content"] = json!({"version":1,"blocks":[
            {"kind":"text","path":"/data/stdout/text"}
        ]});
        let mut page = returned(Presentation::Content, result.clone());
        declare(
            &mut page,
            json!({"label":"Execute","input":{"summary":{
                "path":"/command","format":"command"
            }}}),
        );
        let calls = collect(&page);
        assert!(calls[0].content.is_some());
        assert!(calls[0].resolved.as_ref().unwrap().command.is_none());
        assert!(calls[0].failure.is_none());
        assert!(!calls[0].diagnostics(&page));
        assert_eq!(calls[0].result(&page), Some(&result));
    }
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

#[test]
fn hidden_business_errors_stay_hidden() {
    for result in [
        json!({"error":"business value"}),
        json!({"isError":false,"error":"business value"}),
        json!({"isError":true,"error":"business value"}),
        json!({"isError":false,"structuredContent":{"isError":true,"error":"domain row"}}),
        json!({"output":{"isError":true,"error":"domain row"}}),
        json!({"error":Fault::new(ErrorCode::Unavailable,"business value")}),
    ] {
        for mode in [Presentation::Summary, Presentation::Hidden] {
            let page = returned(mode, result.clone());
            let calls = collect(&page);
            assert_eq!(calls[0].hidden(&page), mode == Presentation::Hidden);
            assert!(!calls[0].diagnostics(&page));
            assert!(!calls[0].details(&page));
            assert_eq!(calls[0].result(&page), Some(&result));
        }
    }
    let result = json!({"error":"Before-tool hook failed"});
    let mut page = returned(Presentation::Hidden, result.clone());
    mark_failure(&mut page, None);
    let calls = collect(&page);
    assert!(!calls[0].hidden(&page));
    assert!(calls[0].diagnostics(&page));
    assert!(calls[0].details(&page));
    assert_eq!(calls[0].result(&page), Some(&result));
}
