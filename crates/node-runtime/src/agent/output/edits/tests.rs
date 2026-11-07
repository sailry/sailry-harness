use super::*;
use sailry_protocol::{FileExcerpt, FileHunk, FileLine};

fn written(excerpt: Option<FileExcerpt>) -> Value {
    json!({"kind":"file_written","data":FileWritten {
        path:"src/lib.rs".into(),revision:"a".repeat(64),size:42,excerpt,
    }})
}

fn hunk(lines: &[(usize, &str)], shortened: bool) -> FileHunk {
    FileHunk {
        lines: lines
            .iter()
            .map(|(number, text)| FileLine {
                number: *number,
                text: (*text).into(),
            })
            .collect(),
        shortened,
    }
}

fn rendered(value: Value) -> CallToolResult {
    let mut messages =
        [Message::user().with_tool_response("edit", Ok(CallToolResult::structured(value)))];
    super::super::project(&mut messages);
    let Content::ToolResult(response) = &messages[0].content[0] else {
        panic!("tool response expected")
    };
    response.result.clone().unwrap()
}

fn text(result: &CallToolResult) -> &str {
    let ContentBlock::Text(text) = &result.content[0] else {
        panic!("text expected")
    };
    &text.text
}

#[test]
fn preserves_result_projection() {
    for (scenario, check) in [
        (
            "numbers_each_edited_region",
            (|scenario: &'static str| {
                let excerpt = FileExcerpt {
                    hunks: vec![
                        hunk(
                            &[(4, "fn a() {\n"), (5, "    changed();\r\n"), (6, "}")],
                            false,
                        ),
                        hunk(&[(40, "tail\n")], true),
                    ],
                    omitted: 2,
                };
                let result = rendered(written(Some(excerpt)));
                let shown = text(&result);
                assert!(shown.starts_with(&format!(
        "src/lib.rs\nEdited; 42 bytes; revision {}\nLines 4–6\n4\tfn a() {{\n5\t    changed();\n6\t}}\n",
        "a".repeat(64)
            )), "{scenario}");
                assert!(
                    shown.contains(
                        "Lines 40–40\n40\ttail\n[Some lines or text in this region are omitted]\n"
                    ),
                    "{scenario}"
                );
                assert!(
                    shown.ends_with("2 more edited regions omitted; read_file shows them\n"),
                    "{scenario}"
                );
                assert!(result.structured_content.is_none(), "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "leaves_whole_file_writes_structured",
            (|scenario: &'static str| {
                let value = written(None);
                let result = rendered(value.clone());
                assert_eq!(result.structured_content, Some(value), "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
