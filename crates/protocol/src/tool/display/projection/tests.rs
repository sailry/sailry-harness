use super::*;
use crate::tool::Display;
use serde_json::json;

fn display(name: &str) -> Display {
    let package: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/files/plugin.json"
    )))
    .unwrap();
    let tools = package["extensions"]["dev.sailry.platform"]["tools"]
        .as_array()
        .unwrap();
    let value = tools.iter().find(|tool| tool["name"] == name).unwrap()["display"].clone();
    let display: Display = serde_json::from_value(value).unwrap();
    assert!(display.valid(), "{name}");
    display
}

#[test]
fn reads_original_large_output_and_joined_rows() {
    let text = "Literal <script> **text** 完整内容 🙂\n".repeat(16_000);
    let result =
        json!({"kind":"office_content","data":{"sections":[{"text":text},{"text":"last"}]}});
    let original = result.clone();
    let projection = display("read_office").output.unwrap();
    assert_eq!(
        projection.preview.unwrap().render(&Value::Null, &result),
        format!("{text}\n\nlast")
    );
    assert_eq!(result, original);
    for (name, result, expected) in [
        (
            "list_directory",
            json!({"data":{"entries":[{"name":"目录","kind":"directory"},{"name":"file.txt","kind":"file"}],"truncated":false,"unsupported_names":0}}),
            "目录/\nfile.txt",
        ),
        (
            "search_files",
            json!({"data":{"matches":[{"path":"目录/a.txt","line_number":42,"line":"  match\t"}]}}),
            "目录/a.txt:42    match\t",
        ),
        (
            "get_office_runtime",
            json!({"data":{"packages":["first==1","second==2"]}}),
            "first==1\nsecond==2",
        ),
        (
            "export_pdf",
            json!({"data":{"file":{"path":"report.pdf"},"warnings":[]}}),
            "report.pdf",
        ),
        (
            "export_pdf",
            json!({"data":{"file":{"path":"report.pdf"},"warnings":["first warning","second warning"]}}),
            "report.pdf\nfirst warning\nsecond warning",
        ),
    ] {
        let projection = display(name).output.unwrap();
        assert_eq!(
            projection.preview.unwrap().render(&Value::Null, &result),
            expected,
            "{name}"
        );
    }
}

#[test]
fn preserves_partial_notices_paths_and_literal_write_inputs() {
    let search = display("search_files").output.unwrap();
    let result = json!({"data":{"matches":[{"path":"one"},{"path":"one"},{"path":"two"}],"truncated":true,"skipped":1}});
    assert_eq!(
        search.notices(&Value::Null, &result, "zh-CN"),
        ["仅显示部分结果"]
    );
    assert_eq!(
        search.paths.unwrap().read(&Value::Null, &result),
        ["one", "one", "two"]
    );
    let read = display("read_file").output.unwrap();
    assert_eq!(
        read.notices(&Value::Null, &json!({"data":{"truncated":false}}), "en"),
        Vec::<String>::new()
    );
    let args = json!({"path":"file.txt","text":"\t<literal>\n","expected_revision":null});
    let body = display("write_file").output.unwrap().body.unwrap();
    assert_eq!(
        body.text.read(&args, &Value::Null).unwrap(),
        "\t<literal>\n"
    );
    let condition = body.diff.unwrap().when.unwrap();
    assert!(condition.matches(&args, &Value::Null));
    assert!(condition.matches(&json!({"text":"new"}), &Value::Null));
    assert!(!condition.matches(&json!({"expected_revision":"current"}), &Value::Null));
}

#[test]
fn prompts_interpolate_once_and_use_captured_arguments() {
    let display = display("export_pdf");
    for (revision, expected) in [
        (Value::Null, "创建 目录/%{path}.pdf"),
        (json!("current"), "覆盖 目录/%{path}.pdf"),
    ] {
        let args = json!({"path":"目录/%{path}.pdf","expected_revision":revision});
        assert_eq!(
            display
                .approval
                .iter()
                .find_map(|prompt| prompt.render(&args, "zh-CN"))
                .unwrap(),
            expected
        );
    }
}

#[test]
fn rejects_ambiguous_and_nested_projection_recipes() {
    for value in [
        json!({"preview":{"parts":[{"text":"x","value":{"source":"result","path":"/text"}}]}}),
        json!({"preview":{"parts":[{"value":{"source":"item","path":""}}]}}),
        json!({"preview":{"parts":[{"value":{"source":"result","path":"/bad~2"}}]}}),
        json!({"notices":[{"message":{"label":"notice"},"when":{"any":[]}}]}),
        json!({"preview":{"parts":[{"rows":{"value":{"source":"result","path":"/rows"},"separator":"\n","text":{"parts":[{"rows":{"value":{"source":"item","path":"/nested"},"separator":"\n","text":{"parts":[{"text":"x"}]}}}]}}}]}}),
    ] {
        let projection: Output = serde_json::from_value(value).unwrap();
        assert!(!projection.valid());
    }
    let prompt: Prompt = serde_json::from_value(
        json!({"message":{"label":"%{path}"},"values":{"path":{"source":"result","path":"/path"}}}),
    )
    .unwrap();
    assert!(
        !prompt.valid(),
        "approval prompts cannot depend on later results"
    );
}

#[test]
fn renders_git_status_columns_and_bounded_commit_ids() {
    let package: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/git/plugin.json"
    )))
    .unwrap();
    let tools = package["extensions"]["dev.sailry.platform"]["tools"]
        .as_array()
        .unwrap();
    let projection = |name| {
        let display: Display = serde_json::from_value(
            tools.iter().find(|tool| tool["name"] == name).unwrap()["display"].clone(),
        )
        .unwrap();
        assert!(display.valid());
        display.output.unwrap()
    };
    let result = json!({"data":{"entries":[
        {"path":"conflict","conflicted":true,"untracked":false,"staged":"modified","unstaged":"deleted"},
        {"path":"new","conflicted":false,"untracked":true,"staged":null,"unstaged":null},
        {"path":"资料.txt","conflicted":false,"untracked":false,"staged":"added","unstaged":"modified"},
        {"path":"gone","conflicted":false,"untracked":false,"staged":null,"unstaged":"deleted"}
    ],"truncated":false,"omitted_paths":0}});
    assert_eq!(
        projection("git_status")
            .preview
            .unwrap()
            .render(&Value::Null, &result),
        "UU  conflict\n??  new\nAM  资料.txt\n D  gone"
    );
    let log = json!({"data":{"entries":[{"id":"0123456789abcdef","author":"Author","message":"literal\nmessage","truncated":true}],"truncated":false,"references_truncated":false}});
    let output = projection("git_log");
    assert_eq!(
        output.preview.as_ref().unwrap().render(&Value::Null, &log),
        "0123456789ab  Author\nliteral\nmessage"
    );
    assert_eq!(
        output.notices(&Value::Null, &log, "en"),
        ["Partial results"]
    );
    let prompt: Prompt = serde_json::from_value(json!({"message":{"label":"prompt"},"when":{"all":[{"value":{"source":"result","path":"/value"},"equals":true}]}})).unwrap();
    assert!(!prompt.valid());
}
