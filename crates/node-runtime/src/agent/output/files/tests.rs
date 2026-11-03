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
    let mut value = original(raw, 7);
    value["data"]["path"] = json!("src/with space/文件🙂.txt");
    let file_revision = value["data"]["revision"].as_str().unwrap();
    let canonical = vec![response(value.clone())];
    let mut outgoing = canonical.clone();
    super::super::project(&mut outgoing);
    let result = result(&outgoing[0]);
    let projected = text(result);
    assert!(projected.starts_with(&format!(
        "src/with space/文件🙂.txt\nLines 8–11; {} bytes; revision {file_revision}\n",
        raw.len()
    )));
    assert_eq!(
        file_revision,
        blake3::hash(raw.as_bytes()).to_hex().as_str()
    );
    assert!(projected.contains("\n8\t\n9\t  trailing spaces  \r\n"));
    assert!(projected.contains("\n10\t\t中文🙂\r\n"));
    assert!(projected.contains("\n11\tlast line [no trailing newline]\n"));
    assert!(result.structured_content.is_none());
    assert_eq!(metadata(projected)["output"], source(&value));
    assert_ne!(metadata(projected)["output"]["revision"], file_revision);
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
        let projected = project(&value, &source(&value), None).unwrap();
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
        let projected = project(&value, &source(&value), None).unwrap();
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
    let projected = project(&value, &source(&value), None).unwrap();
    let prefix = "中".repeat(LINE_LIMIT / 3);
    let omitted = 9000 - prefix.len();
    assert!(projected.contains(&format!(
        "\n1\t{prefix} [... {omitted} content bytes omitted from this line ...]\r\n"
    )));
    assert!(projected.contains("\n2\t\n3\tlast line [no trailing newline]\n"));
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

#[test]
fn reserves_lines_for_continuation_metadata() {
    let value = original(&"line\n".repeat(MAX_LINES), 0);
    let rendered = project(&value, &source(&value), None).unwrap();
    assert!(limit::count(&rendered) <= MAX_LINES);
    let next = metadata(&rendered)["next_offset"].as_u64().unwrap();
    assert!(next > 0 && next < MAX_LINES as u64);
    assert!(rendered.contains(&format!("Next file page: read_file with offset={next}")));
    assert!(rendered.contains(&format!("\n{next}\tline\n")));
}

mod unchanged {
    use super::*;

    const TEXT: &str = "alpha\nbeta\ngamma\ndelta\nepsilon\n";

    fn read(id: &str, value: &Value) -> Message {
        Message::user().with_tool_response(id, Ok(CallToolResult::structured(value.clone())))
    }

    fn window(value: &Value, offset: usize, limit: usize) -> Value {
        let mut window = value.clone();
        let lines = value["data"]["lines"].as_array().unwrap();
        window["data"]["lines"] = json!(lines[offset..offset + limit]);
        window["data"]["offset"] = json!(offset);
        window["data"]["next_offset"] =
            json!((offset + limit < lines.len()).then_some(offset + limit));
        window
    }

    fn shown(messages: &[Message], index: usize) -> String {
        text(result(&messages[index])).to_owned()
    }

    #[test]
    fn stubs_repeated_and_contained_ranges() {
        let full = original(TEXT, 0);
        let revision = full["data"]["revision"].as_str().unwrap().to_owned();
        let mut messages = vec![
            read("first", &full),
            read("again", &full),
            read("inner", &window(&full, 1, 2)),
        ];
        super::super::super::project(&mut messages);
        assert!(shown(&messages, 0).contains("\n3\tgamma\n"));
        for (index, range) in [(1, "Lines 1–5"), (2, "Lines 2–3")] {
            let stub = shown(&messages, index);
            assert!(stub.contains(&format!("{range} are unchanged")), "{stub}");
            assert!(stub.contains(&revision));
            assert!(!stub.contains("gamma"));
        }
        assert!(shown(&messages, 2).contains("Next file page: read_file with offset=3"));
        assert_eq!(metadata(&shown(&messages, 2))["next_offset"], 3);
    }

    #[test]
    fn shows_changed_or_uncovered_text() {
        let first = window(&original(TEXT, 0), 0, 2);
        let changed = original(&TEXT.replace("beta", "BETA"), 0);
        let mut messages = vec![
            read("first", &first),
            read("wider", &window(&original(TEXT, 0), 0, 3)),
            read("changed", &window(&changed, 0, 2)),
        ];
        super::super::super::project(&mut messages);
        assert!(shown(&messages, 1).contains("\n3\tgamma\n"));
        assert!(shown(&messages, 2).contains("\n2\tBETA\n"));
    }

    #[test]
    fn requires_a_visible_complete_source() {
        let full = original(TEXT, 0);
        let mut hidden = read("hidden", &full);
        hidden.metadata = hidden.metadata.with_agent_invisible();
        let mut messages = vec![hidden, read("visible", &full)];
        super::super::super::project(&mut messages);
        assert!(shown(&messages, 1).contains("\n1\talpha\n"));
        let long = format!("{}\nend\n", "x".repeat(LINE_LIMIT + 1));
        let shortened = original(&long, 0);
        let mut messages = vec![read("long", &shortened), read("repeat", &shortened)];
        super::super::super::project(&mut messages);
        assert!(shown(&messages, 1).contains("For omitted line text"));
    }
}
