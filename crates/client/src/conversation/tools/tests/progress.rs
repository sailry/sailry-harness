use super::*;

fn fixture() -> Page {
    let mut page = page();
    let arguments = json!({"title": "Task 中文 🙂", "steps": [
        {"description": "Inspect", "state": "completed"},
        {"description": "Verify", "state": "in_progress"}
    ]});
    page.entries[0].parts[0] = Part::ToolCall {
        display: None,
        presentation: sailry_protocol::tool::Presentation::Progress,
        grouping: Default::default(),
        id: Some("call".into()),
        name: "update_plan".into(),
        arguments: arguments.clone(),
    };
    let mut result = entry(2, page.runs[0].turn, "", true);
    result.parts[0] = Part::ToolResult {
        id: Some("call".into()),
        name: "update_plan".into(),
        result: json!({"progress": arguments}),
        images: vec![],
    };
    page.entries.push(result);
    page
}

#[test]
fn exposes_matched_results() {
    let page = fixture();
    let calls = collect(&page);
    let progress = calls[0].progress.as_ref().unwrap();
    assert_eq!(progress.title.as_deref(), Some("Task 中文 🙂"));
    assert_eq!(progress.steps.len(), 2);
    assert_eq!(
        serde_json::to_value(&calls).unwrap()[0]["progress"]["steps"][1]["state"],
        "in_progress"
    );
    for mutation in 0..7 {
        let mut page = page.clone();
        match mutation {
            0 => {
                page.entries.pop();
            }
            1 => {
                page.entries.remove(0);
            }
            2 => {
                page.entries[1].turn = TurnId::new();
            }
            3 => {
                page.entries[1].branch = "another".into();
            }
            4 => {
                if let Part::ToolResult { name, .. } = &mut page.entries[1].parts[0] {
                    *name = "mcp_update_plan".into();
                }
            }
            5 => {
                if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
                    result["progress"]["steps"][0]["state"] = json!("pending");
                }
            }
            6 => {
                if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
                    result["error"] = json!("not recorded");
                }
            }
            _ => unreachable!(),
        }
        assert!(
            collect(&page).iter().all(|call| call.progress.is_none()),
            "mutation {mutation}"
        );
    }
}

#[test]
fn preserves_invalid_raw_history() {
    let mut page = fixture();
    let bad = json!({"title": null, "steps": [{"description": " ", "state": "pending"}]});
    if let Part::ToolCall { arguments, .. } = &mut page.entries[0].parts[0] {
        *arguments = bad.clone();
    }
    if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
        *result = json!({"progress": bad});
    }
    let calls = collect(&page);
    assert!(calls[0].progress.is_none());
    assert!(calls[0].arguments(&page).is_some() && calls[0].result(&page).is_some());
}

#[test]
fn uses_registered_presentation() {
    let mut page = fixture();
    if let Part::ToolCall { name, .. } = &mut page.entries[0].parts[0] {
        *name = "plugin_example_steps".into();
    }
    if let Part::ToolResult { name, .. } = &mut page.entries[1].parts[0] {
        *name = "plugin_example_steps".into();
    }
    assert!(collect(&page)[0].progress.is_some());
    if let Part::ToolCall { presentation, .. } = &mut page.entries[0].parts[0] {
        *presentation = Default::default();
    }
    assert!(collect(&page)[0].progress.is_none());
}
