use super::*;
use rmcp::model::CallToolResult;

fn response(value: Value) -> Message {
    Message::user().with_tool_response("call", Ok(CallToolResult::structured(value)))
}

fn result(message: &Message) -> &CallToolResult {
    let Content::ToolResult(response) = &message.content[0] else {
        panic!("tool response expected")
    };
    response.result.as_ref().unwrap()
}

fn value(message: &Message) -> Value {
    result(message)
        .structured_content
        .clone()
        .unwrap_or_else(|| {
            serde_json::from_str(result(message).content[0].as_text().unwrap().text.as_str())
                .unwrap()
        })
}

#[test]
fn preserves_result_projection() {
    for (scenario, check) in [
        (
            "normalizes_progress_without_changing_raw",
            (|scenario: &'static str| {
                let progress = (0..100)
                    .map(|percent| format!("\x1b[32m{percent}%\x1b[0m\r"))
                    .collect::<String>();
                let raw = format!(
                    "\x1b[31mCompiling 中文\x1b[0m\r\n{progress}完成🙂\r\n\x1b]8;;https://example.com\x07link\x1b]8;;\x1b\\\n"
                );
                let original = response(json!({"kind":"command_result","data":{
                "stdout":{"text":raw,"truncated":false,"invalid_utf8":false},
                "stderr":{"text":"","truncated":false,"invalid_utf8":false},
                "outcome":{"kind":"exited","data":0},"elapsed_ms":1,
                    }}));
                let mut outgoing = vec![original.clone()];
                project(&mut outgoing);
                assert_eq!(
                    value(&outgoing[0])["data"]["stdout"]["text"],
                    "Compiling 中文\n完成🙂\nlink\n",
                    "{scenario}"
                );
                assert_eq!(
                    value(&original)["data"]["stdout"]["text"],
                    raw,
                    "{scenario}"
                );
                let ContentBlock::Text(text) = &result(&outgoing[0]).content[0] else {
                    panic!("{scenario}: JSON text expected")
                };
                assert_eq!(text.text, value(&outgoing[0]).to_string(), "{scenario}");
                let projected = outgoing.clone();
                project(&mut outgoing);
                assert_eq!(outgoing, projected, "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "bounds_both_stream_tails_at_raw_coordinates",
            (|scenario: &'static str| {
                for kind in ["command_result", "command_output"] {
                    let stdout = format!("HEAD\n{}TAIL", "row 中文🙂\rprogress\n".repeat(4000));
                    let stderr = format!("ERROR {}last failure", "error 中文🙂\n".repeat(4000));
                    let original = json!({"kind":kind,"sailry_command":{"id":"raw"},"data":{
                    "stdout":{"text":stdout,"truncated":false,"invalid_utf8":false},
                    "stderr":{"text":stderr,"truncated":false,"invalid_utf8":true},
                    "outcome":{"kind":"exited","data":7},"elapsed_ms":23,
                        }});
                    let mut outgoing = vec![response(original.clone())];
                    project(&mut outgoing);
                    let projected = value(&outgoing[0]);
                    assert!(projected.to_string().len() <= MAX_BYTES, "{scenario}");
                    let mut lines = 0;
                    for (name, raw, invalid) in
                        [("stdout", &stdout, false), ("stderr", &stderr, true)]
                    {
                        let stream = &projected["data"][name];
                        let text = stream["text"].as_str().unwrap();
                        lines += limit::count(text);
                        let tail = text.split_once('\n').unwrap().1;
                        assert!(terminal::normalize(raw).ends_with(tail), "{scenario}");
                        assert!(
                            text.contains(&format!("read_command id=raw stream={name} offset=0")),
                            "{scenario}"
                        );
                        assert!(text.contains("read_tool_output with output"), "{scenario}");
                        assert_eq!(stream["invalid_utf8"], invalid, "{scenario}");
                        if invalid {
                            assert!(stream["tail_offset"].is_null(), "{scenario}");
                            assert!(stream.get("omitted_bytes").is_none(), "{scenario}");
                        } else {
                            let offset = stream["tail_offset"].as_u64().unwrap() as usize;
                            assert_eq!(terminal::normalize(&raw[offset..]), tail, "{scenario}");
                            assert_eq!(stream["omitted_bytes"], offset, "{scenario}");
                        }
                        assert_eq!(stream["total_bytes"], raw.len(), "{scenario}");
                    }
                    assert!(lines <= MAX_LINES, "{scenario}");
                    assert!(
                        !projected["data"]["stdout"]["text"]
                            .as_str()
                            .unwrap()
                            .contains("HEAD"),
                        "{scenario}"
                    );
                    assert_eq!(
                        projected["data"]["outcome"], original["data"]["outcome"],
                        "{scenario}"
                    );
                    assert_eq!(projected["data"]["elapsed_ms"], 23, "{scenario}");
                    assert_eq!(projected["output"]["call"], "call", "{scenario}");
                    assert_eq!(
                        projected["output"]["revision"],
                        blake3::hash(original.to_string().as_bytes())
                            .to_hex()
                            .to_string(),
                        "{scenario}"
                    );
                    let once = outgoing.clone();
                    project(&mut outgoing);
                    assert_eq!(outgoing, once, "{scenario}");
                }
            }),
        ),
        (
            "preserves_preview_retention_coordinates",
            (|scenario: &'static str| {
                let total = 20 * 1024 * 1024 + 3;
                let head = format!("\x1b[31mHEAD\x1b[0m\n{}", "h".repeat(8192));
                let tail = format!("{}END", "🙂".repeat(2047));
                let tail_offset = total - tail.len();
                for retained_end in [0, 4096, total] {
                    let original = json!({"kind":"command_result","data":{
                    "stdout":{"text":"lossy capture �","truncated":true,"invalid_utf8":true},
                    "stderr":{"text":"","truncated":false,"invalid_utf8":false}},
                    "sailry_command":{"id":"retained","stdout":{
                        "head":head,"head_invalid_utf8":false,"tail":tail,"tail_offset":tail_offset,
                        "total_bytes":total,"retained_bytes":retained_end,"retained_tail_offset":4096,
                        "retained_end":retained_end,"invalid_utf8":false}}
                        });
                    let mut outgoing = vec![response(original.clone())];
                    project(&mut outgoing);
                    let projected = &value(&outgoing[0])["data"]["stdout"];
                    let text = projected["text"].as_str().unwrap();
                    assert!(
                        !text.contains("HEAD") && text.ends_with("END"),
                        "{scenario}"
                    );
                    assert!(projected.get("head_bytes").is_none(), "{scenario}");
                    assert_eq!(projected["omitted_bytes"], tail_offset, "{scenario}");
                    assert_eq!(projected["total_bytes"], total, "{scenario}");
                    assert_eq!(projected["retained_end"], retained_end, "{scenario}");
                    assert_eq!(projected["invalid_utf8"], false, "{scenario}");
                    assert_eq!(
                        text.contains("preview tail starts"),
                        retained_end == total,
                        "{scenario}"
                    );
                    if retained_end == 0 {
                        assert!(!text.contains("read_command"), "{scenario}");
                    } else {
                        let offset = 0;
                        assert!(
                            text.contains(&format!(
                                "read_command id=retained stream=stdout offset={offset}"
                            )),
                            "{scenario}"
                        );
                    }
                }
            }),
        ),
        (
            "preserves_small_unrecognized_results",
            (|scenario: &'static str| {
                let original = vec![
                    response(
                        json!({"kind":"business_record","sailry_command":{"id":"coincidental"},
            "stdout":{"text":"business output"},
            "stderr":{"text":"\u{1b}[31mraw data\u{1b}[0m"}}),
                    ),
                    Message::user().with_tool_response(
                        "mcp",
                        Ok(CallToolResult::success(vec![ContentBlock::text(
                            "unstructured MCP data",
                        )])),
                    ),
                ];
                let mut outgoing = original.clone();
                project(&mut outgoing);
                assert_eq!(outgoing, original, "{scenario}");
            }),
        ),
        (
            "limits_stream_lines_and_escaped_bytes",
            (|scenario: &'static str| {
                for raw in ["\n".repeat(6000), "\\\"".repeat(30000)] {
                    let original = response(json!({"kind":"command_result","data":{
                    "stdout":{"text":raw,"truncated":false},
                    "stderr":{"text":raw,"truncated":false},
                    "outcome":{"kind":"exited","data":1}
                        }}));
                    let mut outgoing = vec![original];
                    project(&mut outgoing);
                    let value = value(&outgoing[0]);
                    assert!(value.to_string().len() <= MAX_BYTES, "{scenario}");
                    let lines = ["stdout", "stderr"]
                        .into_iter()
                        .map(|stream| limit::count(value["data"][stream]["text"].as_str().unwrap()))
                        .sum::<usize>();
                    assert!(lines <= MAX_LINES, "{scenario}");
                    assert_eq!(value["data"]["outcome"]["data"], 1, "{scenario}");
                }
            }),
        ),
        (
            "keeps_unknown_totals_unknown",
            (|scenario: &'static str| {
                let original = response(json!({"kind":"command_result","data":{
                "stdout":{"text":"x".repeat(100_000),"truncated":true,"invalid_utf8":false},
                "stderr":{"text":"","truncated":false},
                    }}));
                let mut outgoing = vec![original];
                project(&mut outgoing);
                let output = value(&outgoing[0]);
                assert!(
                    output["data"]["stdout"]["total_bytes"].is_null(),
                    "{scenario}"
                );
                assert!(
                    output["data"]["stdout"]["text"]
                        .as_str()
                        .unwrap()
                        .contains("total bytes unavailable"),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
