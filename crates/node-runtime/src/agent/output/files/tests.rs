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
        offset,next_offset:None,truncated:false,
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
    let Content::ToolResult(response) = &message.content[0] else {
        panic!("tool response expected")
    };
    response.result.as_ref().unwrap()
}

fn text(result: &CallToolResult) -> &str {
    let ContentBlock::Text(text) = &result.content[0] else {
        panic!("numbered file text expected")
    };
    &text.text
}

#[test]
fn preserves_result_projection() {
    for (scenario, check) in [
        (
            "numbers_text_and_preserves_line_endings",
            (|scenario: &'static str| {
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
            )), "{scenario}");
                assert_eq!(
                    file_revision,
                    blake3::hash(raw.as_bytes()).to_hex().as_str(),
                    "{scenario}"
                );
                assert!(
                    projected.contains("\n8\t\n9\t  trailing spaces  \r\n"),
                    "{scenario}"
                );
                assert!(projected.contains("\n10\t\t中文🙂\r\n"), "{scenario}");
                assert!(
                    projected.contains("\n11\tlast line [no trailing newline]\n"),
                    "{scenario}"
                );
                assert!(result.structured_content.is_none(), "{scenario}");
                assert_eq!(metadata(projected)["output"], source(&value), "{scenario}");
                assert_ne!(
                    metadata(projected)["output"]["revision"],
                    file_revision,
                    "{scenario}"
                );
                assert!(metadata(projected)["next_offset"].is_null(), "{scenario}");
                assert_eq!(
                    self::result(&canonical[0]).structured_content,
                    Some(value),
                    "{scenario}"
                );
                let prepared = outgoing.clone();
                super::super::project(&mut outgoing);
                assert_eq!(outgoing, prepared, "{scenario}");
                let mut reloaded = canonical.clone();
                super::super::project(&mut reloaded);
                assert_eq!(reloaded, prepared, "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "preserves_empty_ranges",
            (|scenario: &'static str| {
                for (offset, size) in [(0, 0), (999, 100)] {
                    let mut value = original("", offset);
                    value["data"]["size"] = json!(size);
                    let projected = project(&value, &source(&value), None).unwrap();
                    assert!(
                        projected.contains(&format!("No lines at offset={offset}; {size} bytes;")),
                        "{scenario}"
                    );
                    assert!(!projected.contains("Lines "), "{scenario}");
                    assert!(!projected.contains("Next file page"), "{scenario}");
                    assert!(metadata(&projected)["next_offset"].is_null(), "{scenario}");
                }
            }),
        ),
        (
            "pages_without_skipping_lines",
            (|scenario: &'static str| {
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
                    assert!(
                        serde_json::to_string(&projected).unwrap().len() <= LIMIT,
                        "{scenario}"
                    );
                    let numbers: Vec<_> = projected
                        .lines()
                        .filter_map(|line| line.split_once('\t')?.0.trim().parse::<usize>().ok())
                        .collect();
                    assert!(!numbers.is_empty(), "{scenario}");
                    assert_eq!(numbers[0], offset + 1, "{scenario}");
                    let last = *numbers.last().unwrap();
                    assert!(
                        projected.contains(&format!("Lines {}–{last};", offset + 1)),
                        "{scenario}"
                    );
                    for number in &numbers {
                        assert!(projected.contains(lines[number - 1].as_str()), "{scenario}");
                    }
                    shown.extend(numbers);
                    let next = metadata(&projected)["next_offset"].as_u64();
                    let Some(next) = next else {
                        assert_eq!(last, lines.len(), "{scenario}");
                        break;
                    };
                    let next = next as usize;
                    assert_eq!(next, last, "{scenario}");
                    assert!(next > offset && next < end, "{scenario}");
                    assert!(
                        projected
                            .contains(&format!("Next file page: read_file with offset={next}")),
                        "{scenario}"
                    );
                    offset = next;
                }
                assert_eq!(shown, (1..=lines.len()).collect::<Vec<_>>(), "{scenario}");
            }),
        ),
        (
            "separates_long_lines_from_file_continuation",
            (|scenario: &'static str| {
                let raw = format!("{}\r\n\nlast line", "中".repeat(3000));
                let value = original(&raw, 0);
                let projected = project(&value, &source(&value), None).unwrap();
                let prefix = "中".repeat(LINE_LIMIT / 3);
                let omitted = 9000 - prefix.len();
                assert!(
                    projected.contains(&format!(
                        "\n1\t{prefix} [... {omitted} content bytes omitted from this line ...]\r\n"
                    )),
                    "{scenario}"
                );
                assert!(
                    projected.contains("\n2\t\n3\tlast line [no trailing newline]\n"),
                    "{scenario}"
                );
                assert!(
                    projected.contains("For omitted line text, use read_tool_output"),
                    "{scenario}"
                );
                assert!(!projected.contains("Next file page"), "{scenario}");
                assert!(metadata(&projected)["next_offset"].is_null(), "{scenario}");
                assert_eq!(metadata(&projected)["output"], source(&value), "{scenario}");
                assert_eq!(
                    value["data"]["lines"][0]["text"],
                    format!("{}\r\n", "中".repeat(3000)),
                    "{scenario}"
                );
            }),
        ),
        (
            "leaves_other_contracts_unchanged",
            (|scenario: &'static str| {
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
                assert_eq!(outgoing, original, "{scenario}");
            }),
        ),
        (
            "removes_only_the_raw_mirror",
            (|scenario: &'static str| {
                let mut original = response(original("source evidence\n", 0));
                let Content::ToolResult(response) = &mut original.content[0] else {
                    unreachable!()
                };
                let result = response.result.as_mut().unwrap();
                let ContentBlock::Text(mirror) = &mut result.content[0] else {
                    unreachable!()
                };
                mirror.text =
                    serde_json::to_string_pretty(result.structured_content.as_ref().unwrap())
                        .unwrap();
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
                assert!(projected.structured_content.is_none(), "{scenario}");
                assert_eq!(projected.is_error, Some(true), "{scenario}");
                assert_eq!(projected.meta, self::result(&original).meta, "{scenario}");
                assert_eq!(
                    &projected.content[1..],
                    &self::result(&original).content[1..],
                    "{scenario}"
                );
                assert_eq!(
                    text(projected).matches("source evidence").count(),
                    1,
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::to_string(projected)
                        .unwrap()
                        .matches("source evidence")
                        .count(),
                    1,
                    "{scenario}"
                );
            }),
        ),
        (
            "reserves_lines_for_continuation_metadata",
            (|scenario: &'static str| {
                let value = original(&"line\n".repeat(MAX_LINES), 0);
                let rendered = project(&value, &source(&value), None).unwrap();
                assert!(limit::count(&rendered) <= MAX_LINES, "{scenario}");
                let next = metadata(&rendered)["next_offset"].as_u64().unwrap();
                assert!(next > 0 && next < MAX_LINES as u64, "{scenario}");
                assert!(
                    rendered.contains(&format!("Next file page: read_file with offset={next}")),
                    "{scenario}"
                );
                assert!(
                    rendered.contains(&format!("\n{next}\tline\n")),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

#[test]
fn preserves_the_exact_byte_ceiling() {
    for (line, ending) in [("x".repeat(1000), "\n"), ("\\\"\t".repeat(166), "\r\n")] {
        let full = |value: &Value| {
            let page: FileLines = serde_json::from_value(value["data"].clone()).unwrap();
            let body: String = page.lines.iter().map(|line| row(line, false).0).collect();
            render(&page, &body, Some((1, 32)), false, None, &source(value))
        };
        let mut raw = format!("{line}{ending}").repeat(32);
        let value = original(&raw, 0);
        let remaining = LIMIT - serde_json::to_string(&full(&value)).unwrap().len();
        assert!(line.len() + remaining < LINE_LIMIT);
        raw.insert_str(raw.len() - ending.len(), &"x".repeat(remaining));
        let value = original(&raw, 0);
        let expected = full(&value);
        assert_eq!(serde_json::to_string(&expected).unwrap().len(), LIMIT);
        assert_eq!(project(&value, &source(&value), None), Some(expected));

        raw.insert(raw.len() - ending.len(), 'x');
        let value = original(&raw, 0);
        assert_eq!(
            serde_json::to_string(&full(&value)).unwrap().len(),
            LIMIT + 1
        );
        let projected = project(&value, &source(&value), None).unwrap();
        let page: FileLines = serde_json::from_value(value["data"].clone()).unwrap();
        let body: String = page.lines[..31]
            .iter()
            .map(|line| row(line, false).0)
            .collect();
        assert_eq!(
            projected,
            render(
                &page,
                &body,
                Some((1, 31)),
                false,
                Some(31),
                &source(&value)
            ),
        );
    }
}

#[test]
fn distinguishes_native_line_truncation() {
    for size in [100, LINE_LIMIT + 100] {
        let mut value = original(&"x".repeat(size), 0);
        value["data"]["truncated"] = json!(true);
        value["data"]["size"] = json!(sailry_protocol::MAX_FILE_BYTES + 100);
        value["data"]["next_offset"] = json!(1);
        let mut reads = Reads::default();
        let projected = project(&value, &source(&value), Some(&mut reads)).unwrap();
        assert!(projected.contains("[source line truncated]"));
        assert!(projected.contains("omitted suffix is not in the saved tool output"));
        assert!(!projected.contains("no trailing newline"));
        assert!(!projected.contains("For omitted line text"));
        assert_eq!(metadata(&projected)["truncated"], true);
        assert_eq!(metadata(&projected)["next_offset"], 1);
        assert_eq!(
            project(&value, &source(&value), Some(&mut reads)),
            Some(projected)
        );
        assert!(reads.0.is_empty());
    }
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
    fn preserves_result_projection() {
        for (scenario, check) in [
            (
                "stubs_repeated_and_contained_ranges",
                (|scenario: &'static str| {
                    let full = original(TEXT, 0);
                    let revision = full["data"]["revision"].as_str().unwrap().to_owned();
                    let mut messages = vec![
                        read("first", &full),
                        read("again", &full),
                        read("inner", &window(&full, 1, 2)),
                    ];
                    super::super::super::project(&mut messages);
                    assert!(shown(&messages, 0).contains("\n3\tgamma\n"), "{scenario}");
                    for (index, range) in [(1, "Lines 1–5"), (2, "Lines 2–3")] {
                        let stub = shown(&messages, index);
                        assert!(
                            stub.contains(&format!("{range} are unchanged")),
                            "{scenario}: {stub}"
                        );
                        assert!(stub.contains(&revision), "{scenario}");
                        assert!(!stub.contains("gamma"), "{scenario}");
                    }
                    assert!(
                        shown(&messages, 2).contains("Next file page: read_file with offset=3"),
                        "{scenario}"
                    );
                    assert_eq!(
                        metadata(&shown(&messages, 2))["next_offset"],
                        3,
                        "{scenario}"
                    );
                }) as fn(&'static str),
            ),
            (
                "shows_changed_or_uncovered_text",
                (|scenario: &'static str| {
                    let first = window(&original(TEXT, 0), 0, 2);
                    let changed = original(&TEXT.replace("beta", "BETA"), 0);
                    let mut messages = vec![
                        read("first", &first),
                        read("wider", &window(&original(TEXT, 0), 0, 3)),
                        read("changed", &window(&changed, 0, 2)),
                    ];
                    super::super::super::project(&mut messages);
                    assert!(shown(&messages, 1).contains("\n3\tgamma\n"), "{scenario}");
                    assert!(shown(&messages, 2).contains("\n2\tBETA\n"), "{scenario}");
                }),
            ),
            (
                "requires_a_visible_complete_source",
                (|scenario: &'static str| {
                    let full = original(TEXT, 0);
                    let mut hidden = read("hidden", &full);
                    hidden.metadata.agent_visible = false;
                    let mut messages = vec![hidden, read("visible", &full)];
                    super::super::super::project(&mut messages);
                    assert!(shown(&messages, 1).contains("\n1\talpha\n"), "{scenario}");
                    let long = format!("{}\nend\n", "x".repeat(LINE_LIMIT + 1));
                    let shortened = original(&long, 0);
                    let mut messages = vec![read("long", &shortened), read("repeat", &shortened)];
                    super::super::super::project(&mut messages);
                    assert!(
                        shown(&messages, 1).contains("For omitted line text"),
                        "{scenario}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
