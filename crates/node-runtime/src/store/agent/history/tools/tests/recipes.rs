use super::*;

fn source(arguments: Value, result: Value, display: Value) -> Raw {
    let mut raw = Raw::new(Some("call".into()), "fixture".into(), arguments);
    raw.result = Some(result);
    raw.presentation = Presentation::Details;
    raw.display = serde_json::from_value(display).unwrap();
    raw
}

fn manifest(package: &str, name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(package)
        .join("plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    manifest["extensions"]["dev.sailry.platform"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap()["display"]
        .clone()
}

#[test]
fn matches_shared_rendering_fixtures() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/tool-display.json",
    )))
    .unwrap();
    assert!(!fixtures.is_empty());
    for fixture in fixtures {
        let raw = source(
            fixture["arguments"].clone(),
            fixture["result"].clone(),
            fixture["display"].clone(),
        );
        assert_eq!(
            serde_json::to_value(raw.detail().resolved).unwrap(),
            fixture["resolved"],
            "{}",
            fixture["name"]
        );
        assert_eq!(raw.detail().arguments, fixture["arguments"]);
        assert_eq!(raw.detail().result, Some(fixture["result"].clone()));
    }
}

#[test]
fn preserves_result_names_and_aliases() {
    let id = "11111111-2222-4333-8444-555555555555";
    for name in [
        Value::Null,
        json!(""),
        json!(" \n "),
        json!(7),
        json!({"name":"ignored"}),
        json!("  计划 🙂.docx  "),
    ] {
        let raw = source(
            json!({"source":{"value":id}}),
            json!({"data":{"path":name}}),
            json!({
                "label":"Read","input":{"summary":{"path":"/source/value","result_path":"/data/path","code":true}}
            }),
        );
        let resolved = raw.detail().resolved.unwrap();
        let expected = name
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(id);
        assert_eq!(resolved.input.summary.unwrap().text, expected);
        assert_eq!(raw.detail().arguments["source"]["value"], id);
    }
    let arguments = json!({"command":"/remote/home/managed/bin/python3 /remote/home/test.py\ncat /remote/home/log.txt"});
    let raw = source(
        arguments.clone(),
        Value::Null,
        json!({
            "label":"Run","input":{"summary":{"path":"/command","code":true,"format":"command"},"content":{"path":"/command","code":true}},
            "aliases":[{"path":"/remote/home","kind":"home"},{"path":"/remote/home/managed/bin/python3","kind":"python"}]
        }),
    );
    let detail = raw.detail();
    let resolved = detail.resolved.unwrap();
    assert_eq!(
        resolved.input.summary.unwrap().text,
        "python ~/test.py\ncat ~/log.txt"
    );
    assert_eq!(resolved.input.content.unwrap().text, arguments["command"]);
    assert_eq!(detail.arguments, arguments);
    assert_eq!(
        raw.summary().resolved.unwrap().input.summary.unwrap().text,
        "python ~/test.py"
    );
}

#[test]
fn bounds_paths_and_retains_complete_details() {
    let mut paths: Vec<_> = (0..40)
        .map(|index| format!("{}file-{index}.txt", "directory/".repeat(30)))
        .collect();
    paths.push(paths[0].clone());
    let raw = source(
        json!({"path":paths[0]}),
        json!({"paths":paths}),
        json!({
            "label":"Read","input":{"summary":{"path":"/path"},"target":{"path":"/path"}},
            "output":{"paths":{"value":{"source":"result","path":"/paths"},"item":""}}
        }),
    );
    let summary = raw.summary();
    let expected = raw.arguments["path"].as_str().unwrap();
    assert!(expected.chars().count() > 200);
    let preview: String = expected.chars().take(200).collect();
    assert_eq!(summary.path.as_deref(), Some(preview.as_str()));
    let resolved = summary.resolved.unwrap();
    assert_eq!(resolved.input.summary.unwrap().text, preview);
    assert_eq!(resolved.input.target.unwrap().text, preview);
    let output = resolved.output.unwrap();
    assert_eq!(output.paths.len(), 32);
    assert_eq!(output.paths[0], preview);
    assert_eq!(output.path_count, 40);
    let detail = raw.detail();
    let resolved = detail.resolved.unwrap();
    assert_eq!(resolved.input.target.unwrap().text, expected);
    let paths = resolved.output.unwrap().paths;
    assert_eq!(paths.len(), 40);
    assert_eq!(paths[0], expected);
    assert_eq!(
        detail.result.unwrap()["paths"].as_array().unwrap().len(),
        41
    );
}

#[test]
fn preserves_captured_connection_and_session_names() {
    for format in ["connection", "session"] {
        let display = json!({"label":"Wait","input":{"summary":{
            "path":"/session","format":format,"names":{"owned":"Captured title"},"result_path":"/name"
        }}});
        for (target, name) in [
            ("owned", Some("Captured title")),
            ("foreign", (format == "connection").then_some("foreign")),
        ] {
            let mut raw = source(json!({"session":target}), Value::Null, display.clone());
            let summary = raw.summary();
            assert_eq!(
                summary
                    .resolved
                    .as_ref()
                    .unwrap()
                    .input
                    .summary
                    .as_ref()
                    .map(|field| field.text.as_str()),
                name
            );
            raw.result = Some(json!({"name":"Saved title"}));
            assert_eq!(
                raw.detail().resolved.unwrap().input.summary.unwrap().text,
                "Saved title"
            );
        }
    }
}

#[test]
fn preserves_batch_edits_and_write_receipts() {
    let arguments = json!({"path":"source.txt","edits":[
        {"old_text":"First","new_text":"Updated first"},
        {"old_text":"Second","new_text":"Updated second"}
    ],"expected_revision":"original"});
    let raw = source(
        arguments.clone(),
        json!({"written":true}),
        manifest("files", "edit_file"),
    );
    let output = raw.detail().resolved.unwrap().output.unwrap();
    assert!(output.body.is_none());
    assert_eq!(output.preview, "Updated first\nUpdated second");
    assert_eq!(output.notices[0].label, "Written");
    assert_eq!(raw.detail().arguments, arguments);
    let result = serde_json::to_value(sailry_protocol::Output::DatabaseOutcome(
        sailry_protocol::database::Outcome::Query(sailry_protocol::database::ResultSet {
            affected_rows: 2,
            ..Default::default()
        }),
    ))
    .unwrap();
    let raw = source(
        Value::Null,
        result.clone(),
        manifest("databases", "database_execute"),
    );
    let output = raw.detail().resolved.unwrap().output.unwrap();
    assert!(output.table.is_none());
    assert_eq!(output.preview, "affected_rows: 2");
    assert_eq!(raw.detail().result, Some(result));
}

#[test]
fn accepts_receipt_names_without_argument_titles() {
    for (name, arguments, result, expected) in [
        (
            "forget_memory",
            json!({"id":"owned"}),
            json!({"receipt":{"title":"Accepted 中文 🙂"}}),
            Some("Accepted 中文 🙂"),
        ),
        (
            "forget_memory",
            json!({"id":"owned"}),
            json!({"status":"pending"}),
            None,
        ),
        (
            "forget_memory",
            json!({"id":"owned"}),
            json!({"receipt":{"title":[119,14,79]}}),
            None,
        ),
        (
            "save_memory",
            json!({"title":"Argument"}),
            json!({"receipt":{"title":" \n "}}),
            Some("Argument"),
        ),
        (
            "save_memory",
            json!({"title":"Argument"}),
            json!({"receipt":{"title":{"id":"internal"}}}),
            Some("Argument"),
        ),
    ] {
        let raw = source(arguments.clone(), result.clone(), manifest("memory", name));
        let resolved = raw.detail().resolved.unwrap();
        assert_eq!(
            resolved
                .input
                .summary
                .as_ref()
                .map(|field| field.text.as_str()),
            expected,
            "{name}"
        );
        assert_eq!(raw.detail().arguments, arguments);
        assert_eq!(raw.detail().result, Some(result));
    }
}

#[test]
fn defers_json_tables_and_bodies_to_detail() {
    let body = "Literal 内容 🙂\n".repeat(16_000);
    let raw = source(
        json!({"action":"invoke"}),
        json!({"data":{"id":"business-id","body":body}}),
        json!({
            "label":"Invoke","output":{"preview":{"parts":[{
                "value":{"source":"result","path":"/data"},"format":"json",
                "when":{"all":[{"value":{"source":"arguments","path":"/action"},"equals":"invoke"}]}
            }]}}
        }),
    );
    let summary = raw.summary();
    assert!(summary.has_details);
    let output = summary.resolved.unwrap().output.unwrap();
    assert!(output.preview.is_empty() && output.preview_deferred);
    assert!(
        !serde_json::to_string(&raw.summary())
            .unwrap()
            .contains("business-id")
    );
    let detail = raw.detail();
    let output = detail.resolved.unwrap().output.unwrap();
    assert_eq!(
        output.preview,
        serde_json::to_string_pretty(&raw.result.as_ref().unwrap()["data"]).unwrap()
    );
    assert!(!output.preview_deferred);
}

#[test]
fn malformed_recipes_preserve_canonical_values() {
    let raw = source(
        json!({"path":"source.rs"}),
        json!({"text":"中文 🙂"}),
        json!({
            "label":"Read","input":{"summary":{"path":"/~2"}}
        }),
    );
    assert!(raw.summary().resolved.is_none());
    assert_eq!(raw.detail().result, raw.result);
    assert_eq!(raw.detail().arguments, raw.arguments);
}

#[test]
fn empty_output_recipes_suppress_literal_fallback() {
    for output in [
        json!({}),
        json!({"body":{"text":{"source":"result","path":"/missing"},"empty":{"label":"Empty"}}}),
    ] {
        let raw = source(
            Value::Null,
            json!("Canonical value"),
            json!({"label":"Read","output":output}),
        );
        assert!(!raw.summary().has_details);
        assert_eq!(raw.detail().result, Some(json!("Canonical value")));
    }
}
