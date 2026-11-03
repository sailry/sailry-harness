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
fn bounds_both_stream_tails_at_raw_coordinates() {
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
        assert!(projected.to_string().len() <= MAX_BYTES);
        let mut lines = 0;
        for (name, raw, invalid) in [("stdout", &stdout, false), ("stderr", &stderr, true)] {
            let stream = &projected["data"][name];
            let text = stream["text"].as_str().unwrap();
            lines += limit::count(text);
            let tail = text.split_once('\n').unwrap().1;
            assert!(terminal::normalize(raw).ends_with(tail));
            assert!(text.contains(&format!("read_command id=raw stream={name} offset=0")));
            assert!(text.contains("read_tool_output with output"));
            assert_eq!(stream["invalid_utf8"], invalid);
            if invalid {
                assert!(stream["tail_offset"].is_null());
                assert!(stream.get("omitted_bytes").is_none());
            } else {
                let offset = stream["tail_offset"].as_u64().unwrap() as usize;
                assert_eq!(terminal::normalize(&raw[offset..]), tail);
                assert_eq!(stream["omitted_bytes"], offset);
            }
            assert_eq!(stream["total_bytes"], raw.len());
        }
        assert!(lines <= MAX_LINES);
        assert!(
            !projected["data"]["stdout"]["text"]
                .as_str()
                .unwrap()
                .contains("HEAD")
        );
        assert_eq!(projected["data"]["outcome"], original["data"]["outcome"]);
        assert_eq!(projected["data"]["elapsed_ms"], 23);
        assert_eq!(projected["output"]["call"], "call");
        assert_eq!(
            projected["output"]["revision"],
            blake3::hash(original.to_string().as_bytes())
                .to_hex()
                .to_string()
        );
        let once = outgoing.clone();
        project(&mut outgoing);
        assert_eq!(outgoing, once);
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
                "retained_end":retained_end,"invalid_utf8":false}}
        });
        let mut outgoing = vec![response(original.clone())];
        project(&mut outgoing);
        let projected = &value(&outgoing[0])["data"]["stdout"];
        let text = projected["text"].as_str().unwrap();
        assert!(!text.contains("HEAD") && text.ends_with("END"));
        assert!(projected.get("head_bytes").is_none());
        assert_eq!(projected["omitted_bytes"], tail_offset);
        assert_eq!(projected["total_bytes"], total);
        assert_eq!(projected["retained_end"], retained_end);
        assert_eq!(projected["invalid_utf8"], false);
        assert_eq!(text.contains("preview tail starts"), retained_end == total);
        if retained_end == 0 {
            assert!(!text.contains("read_command"));
        } else {
            let offset = 0;
            assert!(text.contains(&format!(
                "read_command id=retained stream=stdout offset={offset}"
            )));
        }
    }
}

#[test]
fn preserves_small_unrecognized_results() {
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
    assert_eq!(outgoing, original);
}

#[test]
fn limits_stream_lines_and_escaped_bytes() {
    for raw in ["\n".repeat(6000), "\\\"".repeat(30000)] {
        let original = response(json!({"kind":"command_result","data":{
            "stdout":{"text":raw,"truncated":false},
            "stderr":{"text":raw,"truncated":false},
            "outcome":{"kind":"exited","data":1}
        }}));
        let mut outgoing = vec![original];
        project(&mut outgoing);
        let value = value(&outgoing[0]);
        assert!(value.to_string().len() <= MAX_BYTES);
        let lines = ["stdout", "stderr"]
            .into_iter()
            .map(|stream| limit::count(value["data"][stream]["text"].as_str().unwrap()))
            .sum::<usize>();
        assert!(lines <= MAX_LINES);
        assert_eq!(value["data"]["outcome"]["data"], 1);
    }
}

#[test]
fn keeps_unknown_totals_unknown() {
    let original = response(json!({"kind":"command_result","data":{
        "stdout":{"text":"x".repeat(100_000),"truncated":true,"invalid_utf8":false},
        "stderr":{"text":"","truncated":false},
    }}));
    let mut outgoing = vec![original];
    project(&mut outgoing);
    let output = value(&outgoing[0]);
    assert!(output["data"]["stdout"]["total_bytes"].is_null());
    assert!(
        output["data"]["stdout"]["text"]
            .as_str()
            .unwrap()
            .contains("total bytes unavailable")
    );
}
