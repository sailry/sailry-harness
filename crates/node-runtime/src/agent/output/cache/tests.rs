use super::*;

fn read(call: &str, first: usize, last: usize) -> Message {
    let mut message = Message::user()
        .with_id(format!("message-{call}"))
        .with_tool_response(
            call,
            Ok(CallToolResult::structured(json!({
                "kind":"file_lines",
                "data":{
                    "path":"src/fixture.rs",
                    "lines":(first..=last).map(|number| json!({
                        "number":number,
                        "text":format!("line {number}\n")
                    })).collect::<Vec<_>>(),
                    "size":70,
                    "revision":"a".repeat(64),
                    "offset":first-1,
                    "next_offset":null
                }
            }))),
        );
    message
        .metadata
        .set_operation_note("tool_output", call, json!(format!("revision-{call}")));
    message
}

fn check(cache: &mut Cache, canonical: &[Message]) -> Vec<Message> {
    let mut expected = canonical.to_vec();
    super::super::project(&mut expected);
    let mut actual = canonical.to_vec();
    cache.project(&mut actual);
    assert_eq!(actual, expected);
    actual
}

fn text(message: &Message) -> &str {
    let Content::ToolResult(result) = &message.content[0] else {
        panic!("tool result expected")
    };
    &result.result.as_ref().unwrap().content[0]
        .as_text()
        .unwrap()
        .text
}

#[test]
fn preserves_appended_file_ranges() {
    let mut cache = Cache::default();
    let mut canonical = vec![read("first", 1, 4)];
    check(&mut cache, &canonical);
    canonical.push(Message::assistant().with_text("Read another range"));
    canonical.push(read("contained", 2, 3));
    let projected = check(&mut cache, &canonical);
    assert!(text(&projected[2]).contains("Lines 2–3 are unchanged"));
    check(&mut cache, &canonical);
    canonical.push(read("extended", 3, 5));
    let projected = check(&mut cache, &canonical);
    assert!(text(&projected[3]).contains("5\tline 5\n"));
}

#[test]
fn rebuilds_after_visibility_changes() {
    let mut cache = Cache::default();
    let mut canonical = vec![read("first", 1, 4), read("second", 1, 4)];
    assert!(text(&check(&mut cache, &canonical)[1]).contains("unchanged"));
    canonical[0].metadata.agent_visible = false;
    assert!(text(&check(&mut cache, &canonical)[1]).contains("1\tline 1\n"));
    canonical[0].metadata.agent_visible = true;
    assert!(text(&check(&mut cache, &canonical)[1]).contains("unchanged"));
    canonical.remove(0);
    assert!(text(&check(&mut cache, &canonical)[0]).contains("1\tline 1\n"));
    canonical.clear();
    check(&mut cache, &canonical);
    assert!(cache.entries.is_empty());
}

#[test]
fn rebuilds_changed_revisions_and_media() {
    let mut cache = Cache::default();
    let mut canonical = vec![read("first", 1, 4), read("second", 1, 4)];
    check(&mut cache, &canonical);
    canonical[1].metadata.set_operation_note(
        "tool_output",
        "second",
        json!("replacement-revision"),
    );
    assert!(text(&check(&mut cache, &canonical)[1]).contains("replacement-revision"));
    let Content::ToolResult(response) = &mut canonical[0].content[0] else {
        panic!("tool result expected")
    };
    response
        .result
        .as_mut()
        .unwrap()
        .content
        .push(ContentBlock::image("AQID", "image/png"));
    let first = check(&mut cache, &canonical);
    let Content::ToolResult(response) = &first[0].content[0] else {
        panic!("tool result expected")
    };
    assert!(matches!(
        &response.result.as_ref().unwrap().content[1],
        ContentBlock::Image(_)
    ));
    canonical.swap(0, 1);
    check(&mut cache, &canonical);
}

#[test]
fn retains_bounded_errors_and_commands() {
    let mut canonical = vec![
        Message::user().with_id("failure").with_tool_response(
            "failure",
            Err(rmcp::model::ErrorData::internal_error(
                "Tool failed",
                Some(json!({"details":"x".repeat(MAX_BYTES * 2)})),
            )),
        ),
        Message::user().with_id("command").with_tool_response(
            "command",
            Ok(CallToolResult::structured(json!({
                "kind":"command_result",
                "data":{
                    "stdout":{"text":"\u{1b}[32moutput\u{1b}[0m\r\n".repeat(MAX_LINES * 2),"truncated":false,"invalid_utf8":false},
                    "stderr":{"text":"","truncated":false,"invalid_utf8":false},
                    "outcome":{"kind":"exited","data":0},"elapsed_ms":1
                }
            }))),
        ),
    ];
    for message in &mut canonical {
        let Content::ToolResult(response) = &message.content[0] else {
            panic!("tool result expected")
        };
        message
            .metadata
            .set_operation_note("tool_output", &response.id, json!(response.id));
    }
    let mut cache = Cache::default();
    let first = check(&mut cache, &canonical);
    assert_eq!(check(&mut cache, &canonical), first);
}
