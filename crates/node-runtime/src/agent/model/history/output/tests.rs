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
    assert!(projected["text"].as_str().unwrap().contains("read_command"));
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
    let once = request.clone();
    project(&mut request);
    assert_eq!(request, once);
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
    let canonical = request.clone();
    project(&mut request);
    assert_eq!(request, canonical);
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
fn strips_ansi_and_folds_progress_without_changing_raw() {
    let raw = "\x1b[31mCompiling 中文\x1b[0m\r\n1%\r25%\r完成🙂\r\n\x1b]8;;https://example.com\x07link\x1b]8;;\x1b\\\n";
    let normalized = commands::normalize(raw);
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
}
