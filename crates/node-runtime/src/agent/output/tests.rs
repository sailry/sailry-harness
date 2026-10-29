use super::*;
use rmcp::model::CallToolResult;

fn response(value: Value) -> Message {
    Message::user().with_tool_response("call", Ok(CallToolResult::structured(value)))
}

fn result(message: &Message) -> &CallToolResult {
    let MessageContentBlock::ToolResponse(response) = &message.content[0] else {
        panic!("tool response expected")
    };
    response.tool_result.as_ref().unwrap()
}

fn value(message: &Message) -> &Value {
    result(message).structured_content.as_ref().unwrap()
}

#[test]
fn normalizes_progress_without_changing_raw() {
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
        "Compiling 中文\n完成🙂\nlink\n"
    );
    assert_eq!(value(&original)["data"]["stdout"]["text"], raw);
    let ContentBlock::Text(text) = &result(&outgoing[0]).content[0] else {
        panic!("JSON text expected")
    };
    assert_eq!(text.text, value(&outgoing[0]).to_string());
    let projected = outgoing.clone();
    project(&mut outgoing);
    assert_eq!(outgoing, projected);
}

#[test]
fn bounds_both_streams_at_raw_coordinates() {
    for kind in ["command_result", "command_output"] {
        let stdout = format!(
            "\x1b[31mHEAD\x1b[0m\n{}TAIL",
            "row 中文🙂\rprogress\n".repeat(4000)
        );
        let stderr = format!("ERROR {}last failure", "error 中文🙂\n".repeat(4000));
        let original = json!({"kind":kind,"sailry_command":{"id":"raw"},"data":{
            "stdout":{"text":stdout,"truncated":false,"invalid_utf8":false},
            "stderr":{"text":stderr,"truncated":false,"invalid_utf8":true},
            "outcome":{"kind":"exited","data":7},"elapsed_ms":23,
        }});
        let mut outgoing = vec![response(original.clone())];
        project(&mut outgoing);
        let projected = value(&outgoing[0]);
        assert!(projected.to_string().len() < 36 * 1024);
        for (name, raw, invalid) in [("stdout", &stdout, false), ("stderr", &stderr, true)] {
            let stream = &projected["data"][name];
            let text = stream["text"].as_str().unwrap();
            assert!(text.len() < 18 * 1024);
            let (head, tail) = crate::process::output::edges(raw.as_bytes(), 8192, 8192);
            assert!(text.starts_with(&terminal::normalize(&raw[..head])));
            assert!(text.ends_with(&terminal::normalize(&raw[tail..])));
            assert!(!text.contains('\u{1b}'));
            let offset = if invalid { 0 } else { head };
            assert!(text.contains(&format!(
                "read_command id=raw stream={name} offset={offset}"
            )));
            assert!(text.contains("read_tool_output with output"));
            assert_eq!(stream["invalid_utf8"], invalid);
            if invalid {
                assert!(stream.get("head_bytes").is_none());
                assert!(stream.get("omitted_bytes").is_none());
            } else {
                assert_eq!(stream["head_bytes"], head);
                assert_eq!(stream["tail_offset"], tail);
                assert_eq!(stream["omitted_bytes"], tail - head);
                assert_eq!(stream["total_bytes"], raw.len());
            }
        }
        assert_eq!(projected["data"]["outcome"], original["data"]["outcome"]);
        assert_eq!(projected["data"]["elapsed_ms"], 23);
        assert_eq!(projected["output"]["call"], "call");
        assert_eq!(
            projected["output"]["revision"],
            blake3::hash(original.to_string().as_bytes())
                .to_hex()
                .to_string()
        );
    }
}

#[test]
fn preserves_preview_retention_coordinates() {
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
                "retained_end":retained_end,"invalid_utf8":true}}
        });
        let mut outgoing = vec![response(original.clone())];
        project(&mut outgoing);
        let projected = &value(&outgoing[0])["data"]["stdout"];
        let text = projected["text"].as_str().unwrap();
        assert!(text.starts_with("HEAD\n") && text.ends_with("END"));
        assert_eq!(projected["head_bytes"], 8192);
        assert_eq!(projected["omitted_bytes"], tail_offset - 8192);
        assert_eq!(projected["total_bytes"], total);
        assert_eq!(projected["retained_end"], retained_end);
        assert_eq!(projected["invalid_utf8"], true);
        assert_eq!(text.contains("preview tail starts"), retained_end == total);
        if retained_end == 0 {
            assert!(!text.contains("read_command"));
        } else {
            let offset = if retained_end == total { 8192 } else { 0 };
            assert!(text.contains(&format!(
                "read_command id=retained stream=stdout offset={offset}"
            )));
        }
    }
}

#[test]
fn preserves_other_structured_contracts() {
    let original = vec![
        response(
            json!({"kind":"business_record","sailry_command":{"id":"coincidental"},
            "stdout":{"text":"business output ".repeat(8000)},
            "stderr":{"text":"\u{1b}[31mraw data\u{1b}[0m"}}),
        ),
        Message::user().with_tool_response(
            "mcp",
            Ok(CallToolResult::success(vec![ContentBlock::text(
                "unstructured MCP data ".repeat(8000),
            )])),
        ),
    ];
    let mut outgoing = original.clone();
    project(&mut outgoing);
    assert_eq!(outgoing, original);
}
