use super::*;
use rmcp::model::{CallToolResult, MetaObject};
use sailry_protocol::FileLine;

fn original(text: &str, offset: usize) -> Value {
    json!({"kind":"file_lines","data":FileLines {
        path:"src/fixture.txt".into(),
        lines:text.split_inclusive('\n').enumerate().map(|(index,text)|FileLine {
            number:offset + index + 1,text:text.into(),
        }).collect(),
        size:text.len() as u64,
        revision:blake3::hash(text.as_bytes()).to_hex().to_string(),
        offset,next_offset:None,
    }})
}

fn source(original: &Value) -> Value {
    json!({"call":"file","revision":blake3::hash(original.to_string().as_bytes()).to_hex().to_string()})
}

fn metadata(text: &str) -> Value {
    serde_json::from_str(text.lines().last().unwrap()).unwrap()
}

fn response(value: Value) -> Message {
    Message::user().with_tool_response("file", Ok(CallToolResult::structured(value)))
}

fn result(message: &Message) -> &CallToolResult {
    let MessageContentBlock::ToolResponse(response) = &message.content[0] else {
        panic!("tool response expected")
    };
    response.tool_result.as_ref().unwrap()
}

fn text(result: &CallToolResult) -> &str {
    let ContentBlock::Text(text) = &result.content[0] else {
        panic!("numbered file text expected")
    };
    &text.text
}

#[test]
fn numbers_text_and_preserves_line_endings() {
    let raw = "\n  trailing spaces  \r\n\t中文🙂\r\nlast line";
    let value = original(raw, 7);
    let canonical = vec![response(value.clone())];
    let mut outgoing = canonical.clone();
    super::super::project(&mut outgoing);
    let result = result(&outgoing[0]);
    let projected = text(result);
    assert!(projected.contains("Lines 8–11;"));
    assert!(projected.contains("    8\t\n    9\t  trailing spaces  \r\n"));
    assert!(projected.contains("   10\t\t中文🙂\r\n"));
    assert!(projected.contains("   11\tlast line [no trailing newline]\n"));
    assert!(result.structured_content.is_none());
    assert_eq!(metadata(projected)["output"], source(&value));
    assert!(metadata(projected)["next_offset"].is_null());
    assert_eq!(self::result(&canonical[0]).structured_content, Some(value));
    let prepared = outgoing.clone();
    super::super::project(&mut outgoing);
    assert_eq!(outgoing, prepared);
    let mut reloaded = canonical.clone();
    super::super::project(&mut reloaded);
    assert_eq!(reloaded, prepared);
}

#[test]
fn preserves_empty_ranges() {
    for (offset, size) in [(0, 0), (999, 100)] {
        let mut value = original("", offset);
        value["data"]["size"] = json!(size);
        let projected = project(&value, &source(&value)).unwrap();
        assert!(projected.contains(&format!("No lines at offset={offset}; {size} bytes;")));
        assert!(!projected.contains("Lines "));
        assert!(!projected.contains("Next file page"));
        assert!(metadata(&projected)["next_offset"].is_null());
    }
}

#[test]
fn pages_without_skipping_lines() {
    let lines: Vec<_> = (1..=200)
        .map(|number| format!("{number:04} {}\t中文🙂\r\n", "\\\"".repeat(180)))
        .collect();
    let complete = lines.concat();
    let mut offset = 0;
    let mut shown = Vec::new();
    loop {
        let end = (offset + 100).min(lines.len());
        let mut value = original(&lines[offset..end].concat(), offset);
        value["data"]["size"] = json!(complete.len());
        value["data"]["next_offset"] = json!((end < lines.len()).then_some(end));
        let projected = project(&value, &source(&value)).unwrap();
        assert!(serde_json::to_string(&projected).unwrap().len() <= LIMIT);
        let numbers: Vec<_> = projected
            .lines()
            .filter_map(|line| line.split_once('\t')?.0.trim().parse::<usize>().ok())
            .collect();
        assert!(!numbers.is_empty());
        assert_eq!(numbers[0], offset + 1);
        let last = *numbers.last().unwrap();
        assert!(projected.contains(&format!("Lines {}–{last};", offset + 1)));
        for number in &numbers {
            assert!(projected.contains(lines[number - 1].as_str()));
        }
        shown.extend(numbers);
        let next = metadata(&projected)["next_offset"].as_u64();
        let Some(next) = next else {
            assert_eq!(last, lines.len());
            break;
        };
        let next = next as usize;
        assert_eq!(next, last);
        assert!(next > offset && next < end);
        assert!(projected.contains(&format!("Next file page: read_file with offset={next}")));
        offset = next;
    }
    assert_eq!(shown, (1..=lines.len()).collect::<Vec<_>>());
}

#[test]
fn separates_long_lines_from_file_continuation() {
    let raw = format!("{}\r\n\nlast line", "中".repeat(3000));
    let value = original(&raw, 0);
    let projected = project(&value, &source(&value)).unwrap();
    let prefix = "中".repeat(LINE_LIMIT / 3);
    let omitted = 9000 - prefix.len();
    assert!(projected.contains(&format!(
        "    1\t{prefix} [... {omitted} content bytes omitted from this line ...]\r\n"
    )));
    assert!(projected.contains("    2\t\n    3\tlast line [no trailing newline]\n"));
    assert!(projected.contains("For omitted line text, use read_tool_output"));
    assert!(!projected.contains("Next file page"));
    assert!(metadata(&projected)["next_offset"].is_null());
    assert_eq!(metadata(&projected)["output"], source(&value));
    assert_eq!(
        value["data"]["lines"][0]["text"],
        format!("{}\r\n", "中".repeat(3000))
    );
}

#[test]
fn leaves_other_contracts_unchanged() {
    let value = original("original body\n", 0);
    let mut bare = value["data"].clone();
    bare["unrelated"] = json!(true);
    let mut malformed = value.clone();
    malformed["data"]["lines"] = json!("not a line page");
    let original = vec![
        response(bare),
        response(json!({"kind":"mcp_data","data":value["data"]})),
        response(malformed),
        response(json!({"kind":"tool_output","data":{"text":value.to_string()}})),
    ];
    let mut outgoing = original.clone();
    super::super::project(&mut outgoing);
    assert_eq!(outgoing, original);
}

#[test]
fn removes_only_the_raw_mirror() {
    let mut original = response(original("source evidence\n", 0));
    let MessageContentBlock::ToolResponse(response) = &mut original.content[0] else {
        unreachable!()
    };
    let result = response.tool_result.as_mut().unwrap();
    let ContentBlock::Text(mirror) = &mut result.content[0] else {
        unreachable!()
    };
    mirror.text =
        serde_json::to_string_pretty(result.structured_content.as_ref().unwrap()).unwrap();
    result.is_error = Some(true);
    result.meta = Some(MetaObject(
        serde_json::from_value(json!({"custom":"kept"})).unwrap(),
    ));
    result
        .content
        .push(ContentBlock::image("AQ==", "image/png"));
    result
        .content
        .push(ContentBlock::text("Additional source context"));
    let mut outgoing = vec![original.clone()];
    super::super::project(&mut outgoing);
    let projected = self::result(&outgoing[0]);
    assert!(projected.structured_content.is_none());
    assert_eq!(projected.is_error, Some(true));
    assert_eq!(projected.meta, self::result(&original).meta);
    assert_eq!(
        &projected.content[1..],
        &self::result(&original).content[1..]
    );
    assert_eq!(text(projected).matches("source evidence").count(), 1);
    assert_eq!(
        serde_json::to_string(projected)
            .unwrap()
            .matches("source evidence")
            .count(),
        1
    );
}
