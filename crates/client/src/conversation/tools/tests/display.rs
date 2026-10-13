use super::*;

mod result_summary {
    use super::*;

    const ID: &str = "11111111-2222-4333-8444-555555555555";

    fn fixture(name: Option<Value>, hint: bool) -> Page {
        let mut page = page();
        let mut summary = json!({"path": "/source/value", "code": true});
        if hint {
            summary["result_path"] = json!("/data/path");
        }
        page.entries[0].parts[0] = Part::ToolCall {
            id: Some("call".into()),
            name: "package_read".into(),
            presentation: Default::default(),
            grouping: Default::default(),
            arguments: json!({"source": {"kind": "attachment", "value": ID}}),
            display: Some(
                serde_json::from_value(json!({
                    "label": "Read document", "input": {"summary": summary}
                }))
                .unwrap(),
            ),
        };
        let mut result = json!({"data": {}});
        if let Some(name) = name {
            result["data"]["path"] = name;
        }
        let mut response = entry(2, page.runs[0].turn, "", true);
        response.parts[0] = Part::ToolResult {
            id: Some("call".into()),
            name: "package_read".into(),
            result,
            images: Vec::new(),
        };
        page.entries.push(response);
        page
    }

    fn summary(page: &Page) -> String {
        collect(page)[0]
            .resolved
            .as_ref()
            .unwrap()
            .input
            .summary
            .as_ref()
            .unwrap()
            .text
            .clone()
    }

    #[test]
    fn preserves_names_and_execution_identity() {
        let name = "  计划 🙂.docx  ";
        let page = fixture(Some(json!(name)), true);
        let restored: Page = serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
        assert_eq!(summary(&restored), name);
        let call = collect(&restored).pop().unwrap();
        assert!(
            call.resolved
                .as_ref()
                .unwrap()
                .input
                .summary
                .as_ref()
                .unwrap()
                .code
        );
        assert_eq!(call.id.as_deref(), Some("call"));
        assert_eq!(call.name, "package_read");
        assert_eq!(
            call.arguments(&restored),
            Some(&json!({"source": {"kind": "attachment", "value": ID}}))
        );
        assert_eq!(call.result(&restored).unwrap()["data"]["path"], name);
        assert_eq!(restored, page);
    }

    #[test]
    fn falls_back_without_a_name() {
        for name in [
            None,
            Some(Value::Null),
            Some(json!("")),
            Some(json!(" \n ")),
            Some(json!(7)),
            Some(json!({"name": "ignored"})),
        ] {
            assert_eq!(summary(&fixture(name, true)), ID);
        }
        let mut page = fixture(Some(json!("report.docx")), true);
        page.entries.pop();
        assert_eq!(summary(&page), ID);
    }

    #[test]
    fn preserves_captured_recipes_without_a_hint() {
        let page = fixture(Some(json!("report.docx")), false);
        let captured = serde_json::to_value(&page).unwrap();
        assert!(
            captured["entries"][0]["parts"][0]["data"]["display"]["input"]["summary"]
                .get("result_path")
                .is_none()
        );
        let restored: Page = serde_json::from_value(captured.clone()).unwrap();
        assert_eq!(summary(&restored), ID);
        assert_eq!(serde_json::to_value(restored).unwrap(), captured);
    }

    #[test]
    fn retains_existing_formats() {
        let mut page = fixture(Some(json!("/remote/home/report.docx")), true);
        let Part::ToolCall { display, .. } = &mut page.entries[0].parts[0] else {
            unreachable!();
        };
        let display = display.as_mut().unwrap();
        display
            .input
            .as_mut()
            .unwrap()
            .summary
            .as_mut()
            .unwrap()
            .format = Some(sailry_protocol::tool::InputFormat::Command);
        display.aliases.push(sailry_protocol::tool::Alias {
            path: "/remote/home".into(),
            kind: sailry_protocol::tool::AliasKind::Home,
        });
        assert_eq!(summary(&page), "~/report.docx");
    }
}

mod connection_summary {
    use super::*;

    const ID: &str = "11111111-2222-4333-8444-555555555555";

    fn fixture() -> Page {
        let mut page = page();
        page.entries[0].parts[0] = Part::ToolCall {
            id: Some("call".into()),
            name: "package_run".into(),
            presentation: Default::default(),
            grouping: Default::default(),
            arguments: json!({"connection": ID, "command": "uname -a"}),
            display: Some(
                serde_json::from_value(json!({
                    "label": "Run", "input": {"summary": {
                        "path": "/connection", "format": "connection",
                        "result_path": "/connection_name", "names": {ID: "Build host"}
                    }}
                }))
                .unwrap(),
            ),
        };
        page
    }

    fn summary(page: &Page) -> String {
        collect(page)[0]
            .resolved
            .as_ref()
            .unwrap()
            .input
            .summary
            .as_ref()
            .unwrap()
            .text
            .clone()
    }

    #[test]
    fn retains_admitted_names_in_every_state() {
        let mut page = fixture();
        assert_eq!(summary(&page), "Build host");
        assert_eq!(
            collect(&page)[0].arguments(&page).unwrap()["connection"],
            ID
        );
        for result in [
            json!({"isError":true,"error":{"code":"unavailable","message":"Unavailable"}}),
            json!({"kind":"ssh_outcome","data":{"kind":"command","stdout":"ready"}}),
        ] {
            let mut response = entry(2, page.runs[0].turn, "", true);
            response.parts[0] = Part::ToolResult {
                id: Some("call".into()),
                name: "package_run".into(),
                result,
                images: Vec::new(),
            };
            page.entries.truncate(1);
            page.entries.push(response);
            let restored: Page =
                serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
            assert_eq!(summary(&restored), "Build host");
            assert_eq!(
                collect(&restored)[0].arguments(&restored).unwrap()["connection"],
                ID
            );
            assert_eq!(restored, page);
        }
    }

    #[test]
    fn falls_back_and_accepts_result_names() {
        for names in [
            json!({}),
            json!({"unknown":"Other host"}),
            json!({ID:" \n "}),
        ] {
            let mut page = fixture();
            let Part::ToolCall { display, .. } = &mut page.entries[0].parts[0] else {
                unreachable!()
            };
            display
                .as_mut()
                .unwrap()
                .input
                .as_mut()
                .unwrap()
                .summary
                .as_mut()
                .unwrap()
                .names = serde_json::from_value(names).unwrap();
            assert_eq!(summary(&page), ID);
        }
        let mut page = fixture();
        let mut response = entry(2, page.runs[0].turn, "", true);
        response.parts[0] = Part::ToolResult {
            id: Some("call".into()),
            name: "package_run".into(),
            result: json!({"connection_name":"Saved host"}),
            images: Vec::new(),
        };
        page.entries.push(response);
        assert_eq!(summary(&page), "Saved host");
    }
}

mod session_summary {
    use super::*;
    use sailry_protocol::{Delegation, conversation::Child};

    fn fixture(title: Option<&str>, name: Option<&str>) -> Page {
        let mut page = page();
        let session = SessionId::new();
        let origin = Delegation {
            session: page.session,
            turn: page.entries[0].turn,
            entry: page.entries[0].id.clone(),
            index: 0,
            role: None,
        };
        let Part::ToolCall { arguments, .. } = &mut page.entries[0].parts[0] else {
            unreachable!();
        };
        *arguments = title
            .map(|title| json!({"title": title}))
            .unwrap_or(json!({}));
        let mut run = page.runs[0].clone();
        run.session = session;
        run.turn = TurnId::new();
        page.children.push(Child {
            run,
            origin,
            name: name.map(str::to_owned),
        });
        let mut wait = entry(2, page.runs[0].turn, "", false);
        wait.parts[0] = Part::ToolCall {
            id: Some("wait".into()),
            name: "package_wait".into(),
            presentation: Default::default(),
            grouping: Default::default(),
            arguments: json!({"session": session.to_string()}),
            display: Some(
                serde_json::from_value(json!({
                    "label": "Wait for subagent",
                    "input": {"summary": {"path": "/session", "format": "session"}}
                }))
                .unwrap(),
            ),
        };
        page.entries.push(wait);
        page
    }

    fn summary(page: &Page) -> Option<String> {
        collect(page)
            .last()?
            .resolved
            .as_ref()?
            .input
            .summary
            .as_ref()
            .map(|text| text.text.clone())
    }

    #[test]
    fn prefers_the_canonical_spawn_title() {
        let page = fixture(Some("检查布局 🙂"), Some("Reviewer"));
        let restored: Page = serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
        assert_eq!(summary(&restored).as_deref(), Some("检查布局 🙂"));
        let call = collect(&restored).pop().unwrap();
        assert_eq!(
            call.arguments(&restored).unwrap()["session"],
            page.children[0].run.session.to_string()
        );
    }

    #[test]
    fn prefixes_only_configured_role_names() {
        let mut page = fixture(Some("检查布局 🙂"), Some("Reviewer"));
        page.children[0].origin.role = Some(sailry_protocol::RoleId::new());
        assert_eq!(summary(&page).as_deref(), Some("Reviewer 检查布局 🙂"));
        page.children[0].origin.role = None;
        assert_eq!(summary(&page).as_deref(), Some("检查布局 🙂"));
        page.children[0].origin.role = Some(sailry_protocol::RoleId::new());
        page.children[0].name = Some(" ".into());
        assert_eq!(summary(&page).as_deref(), Some("检查布局 🙂"));
        let Part::ToolCall { arguments, .. } = &mut page.entries[0].parts[0] else {
            unreachable!();
        };
        *arguments = json!({});
        assert_eq!(summary(&page), None);
    }

    #[test]
    fn falls_back_to_frozen_child_metadata() {
        for title in [None, Some(" ")] {
            let mut page = fixture(title, Some("Reviewer"));
            assert_eq!(summary(&page).as_deref(), Some("Reviewer"));
            page.children[0].origin.entry = "unloaded-origin".into();
            assert_eq!(summary(&page).as_deref(), Some("Reviewer"));
        }
    }

    #[test]
    fn omits_missing_titles_and_foreign_sessions() {
        for name in [None, Some(" ")] {
            let page = fixture(None, name);
            assert_eq!(summary(&page), None);
            assert_eq!(
                collect(&page)
                    .last()
                    .unwrap()
                    .resolved
                    .as_ref()
                    .unwrap()
                    .label
                    .as_ref()
                    .unwrap()
                    .label,
                "Wait for subagent"
            );
        }
        let mut page = fixture(Some("Known task"), None);
        page.children[0].run.session = SessionId::new();
        assert_eq!(summary(&page), None);
        page.children.clear();
        assert_eq!(summary(&page), None);
    }

    #[test]
    fn requires_the_exact_canonical_origin() {
        let mut page = fixture(Some("Known task"), None);
        page.children[0].origin.turn = TurnId::new();
        assert_eq!(summary(&page), None);
        page.children[0].origin.turn = page.entries[0].turn;
        page.children[0].origin.index = 1;
        assert_eq!(summary(&page), None);
    }

    #[test]
    fn preserves_captured_unformatted_recipes() {
        let mut page = fixture(Some("Known task"), None);
        let Part::ToolCall { display, .. } = &mut page.entries[1].parts[0] else {
            unreachable!();
        };
        display
            .as_mut()
            .unwrap()
            .input
            .as_mut()
            .unwrap()
            .summary
            .as_mut()
            .unwrap()
            .format = None;
        let restored: Page = serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
        assert_eq!(
            summary(&restored),
            Some(page.children[0].run.session.to_string())
        );
        assert_eq!(restored, page);
    }
}

#[test]
fn formats_summary_without_changing_raw_arguments() {
    let mut page = page();
    let arguments = json!({"command":"/remote/home/managed/bin/python3 /remote/home/test.py\ncat /remote/home/log.txt"});
    page.entries[0].parts[0] = Part::ToolCall {
        id: Some("call".into()), name: "package_probe".into(),
        presentation: Default::default(), grouping: Default::default(),
        arguments: arguments.clone(),
        display: Some(serde_json::from_value(json!({
            "label":"Run", "input":{"summary":{"path":"/command","code":true,"format":"command"},"content":{"path":"/command","code":true}},
            "aliases":[{"path":"/remote/home","kind":"home"},{"path":"/remote/home/managed/bin/python3","kind":"python"}]
        })).unwrap()),
    };
    let restored: Page = serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
    let calls = collect(&restored);
    let resolved = calls[0].resolved.as_ref().unwrap();
    assert_eq!(
        resolved.input.summary.as_ref().unwrap().text,
        "python ~/test.py\ncat ~/log.txt"
    );
    assert_eq!(
        resolved.input.content.as_ref().unwrap().text,
        arguments["command"]
    );
    assert_eq!(calls[0].arguments(&restored), Some(&arguments));
}

#[test]
fn matches_the_flutter_contract() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/tool-display.json"
    ))
    .unwrap();
    assert!(!fixtures.is_empty());
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
