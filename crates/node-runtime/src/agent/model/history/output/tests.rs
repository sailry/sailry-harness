use super::*;
use adk_core::FunctionResponseData;

fn response(name: &str, value: Value) -> Content {
    Content {
        role: "function".into(),
        parts: vec![Part::FunctionResponse {
            function_response: FunctionResponseData::with_inline_data(
                name,
                value,
                vec![adk_core::InlineDataPart {
                    mime_type: "image/png".into(),
                    data: vec![1, 2, 3],
                    uri: None,
                    annotations: None,
                }],
            ),
            id: Some("call".into()),
            annotations: Some(json!({"preserved":true})),
        }],
    }
}

fn value(content: &Content) -> &Value {
    let Part::FunctionResponse {
        function_response, ..
    } = &content.parts[0]
    else {
        unreachable!()
    };
    &function_response.response
}

#[test]
fn bounds_mcp_without_rewriting_history() {
    let output = json!({"text":[format!("begin\n{}\nfinal failure 中文🙂", "日志🙂".repeat(60_000))],"isError":true});
    let canonical = vec![response("mcp_fixture_logs", output.clone())];
    let mut request = canonical.clone();
    project(&mut request);
    let projected = value(&request[0]);
    assert_eq!(value(&canonical[0]), &output);
    assert!(
        projected["text"]
            .as_str()
            .unwrap()
            .contains("final failure 中文🙂")
    );
    assert!(projected["text"].as_str().unwrap().contains("begin"));
    assert!(
        projected["text"]
            .as_str()
            .unwrap()
            .contains("bytes omitted")
    );
    assert!(
        projected["text"]
            .as_str()
            .unwrap()
            .contains("read_tool_output")
    );
    assert_eq!(projected["isError"], true);
    assert_eq!(projected["output"]["call"], "call");
    assert_eq!(
        projected["output"]["revision"],
        blake3::hash(output.to_string().as_bytes())
            .to_hex()
            .to_string()
    );
    let Part::FunctionResponse {
        function_response,
        annotations,
        ..
    } = &request[0].parts[0]
    else {
        unreachable!()
    };
    assert_eq!(function_response.inline_data[0].data, vec![1, 2, 3]);
    assert_eq!(annotations, &Some(json!({"preserved":true})));
    let once = serde_json::to_value(&request).unwrap();
    project(&mut request);
    assert_eq!(serde_json::to_value(&request).unwrap(), once);
}

#[test]
fn leaves_small_and_unrecognized_results_unchanged() {
    let mut request = vec![
        response("mcp_fixture_logs", json!({"text":["small"]})),
        response("plugin_fixture", json!({"value":"x".repeat(LIMIT * 2)})),
        response(
            "plugin_files_read",
            json!({"kind":"file_lines","data":{"path":"file"}}),
        ),
    ];
    let canonical = serde_json::to_value(&request).unwrap();
    project(&mut request);
    assert_eq!(serde_json::to_value(&request).unwrap(), canonical);
}

#[test]
fn reuses_only_the_loaded_guide_revision() {
    let body = "Current guide instructions\n".repeat(200);
    let output = json!({
        "kind":"file_lines",
        "data":{"path":"AGENTS.md","revision":"current","size":body.len(),"offset":0,"next_offset":null,
            "lines":[{"number":1,"text":body}]},
        "sailry_guide_loaded":{"path":"AGENTS.md","revision":"current"},
    });
    let canonical = vec![response("plugin_files_read", output.clone())];
    let mut request = canonical.clone();
    project(&mut request);
    assert_eq!(value(&request[0])["already_loaded"], true);
    assert!(
        value(&request[0])["text"]
            .as_str()
            .unwrap()
            .contains("read_tool_output with output")
    );
    assert_eq!(value(&canonical[0]), &output);
    let mut changed = output;
    changed["data"]["revision"] = json!("changed");
    changed["sailry_context"] = json!("Newly discovered guide context");
    let mut request = vec![response("plugin_files_read", changed)];
    project(&mut request);
    assert!(value(&request[0]).get("already_loaded").is_none());
    assert!(
        value(&request[0])["text"]
            .as_str()
            .unwrap()
            .contains("Current guide instructions")
    );
    assert_eq!(
        value(&request[0])["sailry_context"],
        "Newly discovered guide context"
    );
}

#[test]
fn numbers_file_text_with_bounded_lines() {
    let lines: Vec<_> = (1..=100)
        .map(|number| sailry_protocol::FileLine {
            number,
            text: if number == 20 {
                "长行🙂".repeat(1000)
            } else {
                format!("const value_{number} = '中文🙂';\n")
            },
        })
        .collect();
    let original = json!({"kind":"file_lines","data":sailry_protocol::FileLines {
        path:"src/fixture.rs".into(),lines,size:100_000,revision:"revision".into(),offset:0,next_offset:Some(100),
    }});
    let mut request = vec![response("plugin_files_read", original.clone())];
    project(&mut request);
    let projected = value(&request[0]);
    let text = projected["text"].as_str().unwrap();
    assert!(text.starts_with("src/fixture.rs\nLines 1–100; 100000 bytes; revision revision\n"));
    assert!(text.contains("    1\tconst value_1"));
    assert!(text.contains("bytes omitted"));
    assert!(text.contains("offset=100"));
    assert!(text.len() < LIMIT);
    assert!(projected.to_string().len() < original.to_string().len());
}

#[test]
fn projects_unwrapped_file_operation_results() {
    let original = json!({"path":"source.txt","lines":[{"number":1,"text":"first ".repeat(800)}],
        "size":4800,"revision":"canonical-revision","offset":0,"next_offset":null});
    let mut request = vec![response("declared_file_reader", original.clone())];
    project(&mut request);
    assert!(
        value(&request[0])["text"]
            .as_str()
            .unwrap()
            .starts_with("source.txt\nLines 1–1")
    );
    assert!(
        value(&request[0])["text"]
            .as_str()
            .unwrap()
            .contains("bytes omitted")
    );
    assert_eq!(
        value(&request[0])["output"]["revision"],
        blake3::hash(original.to_string().as_bytes())
            .to_hex()
            .to_string()
    );
}

#[test]
fn groups_search_and_keeps_match_context() {
    let long = format!("{}needle中文🙂{}", "前".repeat(1000), "后".repeat(1000));
    let rows = (1..=12)
        .map(|line_number| sailry_protocol::SearchMatch {
            path: "a/long/path/to/fixture.rs".into(),
            line_number,
            line: long.clone(),
            matched: true,
            truncated: false,
        })
        .collect();
    let output = json!({"kind":"search_results","data":sailry_protocol::SearchResults { matches:rows, ..Default::default() }});
    let call = Content {
        role: "model".into(),
        parts: vec![Part::FunctionCall {
            name: "plugin_files_search".into(),
            args: json!({"query":"needle中文"}),
            id: Some("call".into()),
            thought_signature: None,
        }],
    };
    let mut request = vec![call, response("plugin_files_search", output)];
    project(&mut request);
    let text = value(&request[1])["text"].as_str().unwrap();
    assert_eq!(text.matches("a/long/path/to/fixture.rs").count(), 1);
    assert_eq!(text.matches("needle中文🙂").count(), 12);
    assert!(text.chars().count() < 4_000);
}

#[test]
fn locates_matches_without_decoding_other_arguments() {
    let line = format!(
        "{}needle中文🙂{}",
        "before ".repeat(200),
        " after".repeat(200)
    );
    let output = json!({"kind":"search_results","data":sailry_protocol::SearchResults {
        matches:vec![sailry_protocol::SearchMatch {path:"fixture.rs".into(),line_number:1,
            line,matched:true,truncated:false}], ..Default::default()
    }});
    let call = Content {
        role: "model".into(),
        parts: vec![Part::FunctionCall {
            name: "plugin_files_search".into(),
            args: json!({"query":"needle中文","limit":"unused metadata"}),
            id: Some("call".into()),
            thought_signature: None,
        }],
    };
    let mut request = vec![call, response("plugin_files_search", output)];
    project(&mut request);
    let text = value(&request[1])["text"].as_str().unwrap();
    assert!(text.contains("needle中文🙂"));
    assert!(text.chars().count() < 400);
}

#[test]
fn normalizes_progress_without_changing_raw() {
    let progress = (0..100)
        .map(|percent| format!("\x1b[32m{percent}%\x1b[0m\r"))
        .collect::<String>();
    let raw = format!(
        "\x1b[31mCompiling 中文\x1b[0m\r\n{progress}完成🙂\r\n\x1b]8;;https://example.com\x07link\x1b]8;;\x1b\\\n"
    );
    let normalized = commands::normalize(&raw);
    assert_eq!(normalized, "Compiling 中文\n完成🙂\nlink\n");
    let output = json!({"kind":"command_result","data":{
        "stdout":{"text":raw,"truncated":false,"invalid_utf8":false},
        "stderr":{"text":"","truncated":false,"invalid_utf8":false},
        "outcome":{"kind":"exited","data":0},"elapsed_ms":1,
    }});
    let canonical = vec![response("plugin_commands_run", output)];
    let mut request = canonical.clone();
    project(&mut request);
    assert_eq!(value(&request[0])["data"]["stdout"]["text"], normalized);
    assert_eq!(value(&canonical[0])["data"]["stdout"]["text"], raw);
    assert!(
        serde_json::to_vec(&request).unwrap().len() < serde_json::to_vec(&canonical).unwrap().len()
    );
}

#[test]
fn bounds_complete_command_diagnostics() {
    for command in ["cargo test", "fixture-command"] {
        for wrapped in [false, true] {
            let data = json!({
                "info":{"command":command,"id":"00000000-0000-4000-8000-000000000001"},
                "stdout":{"text":format!("failures:\n{}\nlast failure", "diagnostic row\n".repeat(4000)),
                    "truncated":false,"invalid_utf8":false},
                "stderr":{"text":"","truncated":false,"invalid_utf8":false}
            });
            let original = if wrapped {
                json!({"kind":"command_result","data":data})
            } else {
                data
            };
            let canonical = vec![response("plugin_commands_run", original.clone())];
            let mut request = canonical.clone();
            project(&mut request);
            let projected = value(&request[0]);
            assert!(projected["text"].as_str().unwrap().len() < LIMIT + 1024);
            assert!(projected["text"].as_str().unwrap().contains("last failure"));
            assert!(
                projected["text"]
                    .as_str()
                    .unwrap()
                    .contains("read_command id=00000000-0000-4000-8000-000000000001")
            );
            assert_eq!(projected["output"]["call"], "call");
            assert_eq!(value(&canonical[0]), &original);
        }
    }
}

#[test]
fn shortens_log_without_changing_canonical_entries() {
    let output = json!({"kind":"git_log","data":{"entries":[{
        "id":"commit","author":"Author","email":"private@example.test","timestamp":123,
        "message":"Title\n\nFirst body\nSecond body\nThird body\nFourth body\nSigned-off-by: Author <private@example.test>\nCo-authored-by: Other <other@example.test>"
    }]}});
    let canonical = vec![response("plugin_git_log", output.clone())];
    let mut request = canonical.clone();
    project(&mut request);
    let entry = &value(&request[0])["data"]["entries"][0];
    assert_eq!(
        entry["message"],
        "Title\nFirst body\nSecond body\nThird body"
    );
    assert!(entry.get("email").is_none());
    assert!(entry.get("timestamp").is_none());
    assert_eq!(value(&canonical[0]), &output);
}

#[test]
fn removes_only_sparse_status_fields() {
    let mut entries = vec![json!({"path":"zero.txt","binary":false,"added":0,"deleted":3})];
    entries.extend((1..16).map(
        |index| json!({"path":format!("source_{index}.rs"),"binary":false,"added":0,"deleted":3}),
    ));
    let output = json!({"kind":"git_status","data":{
        "staged":false,"deleted":null,"ahead":0,"behind":2,"dirty":true,
        "entries":entries,
        "message":"","literal":"false",
    }});
    let canonical = vec![response("plugin_git_status", output.clone())];
    let mut request = canonical.clone();
    project(&mut request);
    let data = &value(&request[0])["data"];
    for key in ["staged", "deleted", "ahead"] {
        assert!(data.get(key).is_none());
    }
    assert_eq!(data["behind"], 2);
    assert_eq!(data["dirty"], true);
    assert_eq!(data["message"], "");
    assert_eq!(data["literal"], "false");
    assert_eq!(data["entries"][0], json!({"path":"zero.txt","deleted":3}));
    assert_eq!(data["entries"].as_array().unwrap().len(), 16);
    for index in 1..16 {
        assert_eq!(
            data["entries"][index],
            json!({"path":format!("source_{index}.rs"),"deleted":3})
        );
    }
    assert_eq!(value(&canonical[0]), &output);
    assert!(
        serde_json::to_vec(&request).unwrap().len() < serde_json::to_vec(&canonical).unwrap().len()
    );
}
