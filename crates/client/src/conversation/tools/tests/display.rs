use super::*;

#[test]
fn matches_the_flutter_contract() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/tool-display.json"
    ))
    .unwrap();
    assert_eq!(fixtures.len(), 5);
    for fixture in fixtures {
        let mut page = page();
        let name = fixture["name"].as_str().unwrap();
        page.entries[0].parts[0] = Part::ToolCall {
            id: Some("call".into()),
            name: name.into(),
            presentation: Default::default(),
            grouping: Default::default(),
            arguments: fixture["arguments"].clone(),
            display: serde_json::from_value(fixture["display"].clone()).unwrap(),
        };
        let mut response = entry(2, page.runs[0].turn, "", true);
        response.parts[0] = Part::ToolResult {
            id: Some("call".into()),
            name: name.into(),
            result: fixture["result"].clone(),
            images: vec![],
        };
        page.entries.push(response);
        let restored: Page = serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
        let calls = collect(&restored);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            serde_json::to_value(&calls[0].resolved).unwrap(),
            fixture["resolved"],
            "{name}"
        );
        assert_eq!(calls[0].arguments(&restored), Some(&fixture["arguments"]));
        assert_eq!(calls[0].result(&restored), Some(&fixture["result"]));
    }
}

#[test]
fn malformed_recipes_do_not_replace_raw_history() {
    let mut page = page();
    let Part::ToolCall { display, .. } = &mut page.entries[0].parts[0] else {
        panic!("call expected");
    };
    *display = Some(
        serde_json::from_value(json!({
            "label":"Read", "input":{"summary":{"path":"/~2"}}
        }))
        .unwrap(),
    );
    page.entries.push(entry(2, page.runs[0].turn, "", true));
    let calls = collect(&page);
    assert!(calls[0].resolved.is_none());
    assert_eq!(calls[0].result(&page), Some(&json!({"text":"中文 🙂"})));
}
