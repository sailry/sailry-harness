use super::*;
use rmcp::model::CallToolResult;
use sailry_protocol::{SearchCount, SearchDiagnostic, SearchMatch};

fn original(page: SearchResults) -> Value {
    json!({"kind":"search_results","data":page})
}

fn source(value: &Value) -> Value {
    json!({"call":"search","revision":blake3::hash(value.to_string().as_bytes()).to_hex().to_string()})
}

fn metadata(text: &str) -> Value {
    serde_json::from_str(text.lines().last().unwrap()).unwrap()
}

fn message(value: Value) -> Message {
    Message::user().with_tool_response("search", Ok(CallToolResult::structured(value)))
}

fn result(message: &Message) -> &CallToolResult {
    let Content::ToolResult(response) = &message.content[0] else {
        panic!("tool response expected")
    };
    response.result.as_ref().unwrap()
}

fn text(result: &CallToolResult) -> &str {
    let ContentBlock::Text(text) = &result.content[0] else {
        panic!("search text expected")
    };
    &text.text
}

fn row(number: usize, line: impl Into<String>) -> SearchMatch {
    SearchMatch {
        path: "src/文件🙂.rs".into(),
        line_number: number,
        line: line.into(),
        matched: number.is_multiple_of(2),
        truncated: false,
    }
}

#[test]
fn preserves_result_projection() {
    for (scenario, check) in [
        (
            "preserves_modes_and_exact_source",
            (|scenario: &'static str| {
                for page in [
                    SearchResults {
                        matches: vec![row(1, ""), row(2, "  needle\t中文🙂  ")],
                        scanned_files: 3,
                        ..Default::default()
                    },
                    SearchResults {
                        mode: SearchMode::Files,
                        paths: vec!["src/with space/文件🙂.rs".into(), "line\nbreak.rs".into()],
                        scanned_files: 3,
                        ..Default::default()
                    },
                    SearchResults {
                        mode: SearchMode::Count,
                        counts: vec![SearchCount {
                            path: "src/文件🙂.rs".into(),
                            count: 9,
                        }],
                        scanned_files: 3,
                        ..Default::default()
                    },
                ] {
                    let value = original(page.clone());
                    let canonical = vec![message(value.clone())];
                    let mut outgoing = canonical.clone();
                    super::super::project(&mut outgoing);
                    let projected = result(&outgoing[0]);
                    assert!(projected.structured_content.is_none(), "{scenario}");
                    let rendered = text(projected);
                    assert_eq!(metadata(rendered)["output"], source(&value), "{scenario}");
                    assert_eq!(metadata(rendered)["partial"], false, "{scenario}");
                    assert!(metadata(rendered)["next_offset"].is_null(), "{scenario}");
                    match page.mode {
                        SearchMode::Content => {
                            assert!(
                                rendered.contains("src/文件🙂.rs\n1-\t\n2:\t  needle\t中文🙂  \n"),
                                "{scenario}"
                            );
                            assert_eq!(rendered.matches("src/文件🙂.rs").count(), 1, "{scenario}");
                        }
                        SearchMode::Files => {
                            assert!(
                                rendered
                                    .contains("src/with space/文件🙂.rs\n\"line\\nbreak.rs\"\n"),
                                "{scenario}"
                            );
                        }
                        SearchMode::Count => {
                            assert!(rendered.contains("9\tsrc/文件🙂.rs\n"), "{scenario}")
                        }
                    }
                    assert_eq!(
                        result(&canonical[0]).structured_content,
                        Some(value),
                        "{scenario}"
                    );
                    let prepared = outgoing.clone();
                    super::super::project(&mut outgoing);
                    assert_eq!(outgoing, prepared, "{scenario}");
                    let mut reloaded = canonical;
                    super::super::project(&mut reloaded);
                    assert_eq!(reloaded, prepared, "{scenario}");
                }
            }) as fn(&'static str),
        ),
        (
            "pages_from_the_last_shown_row",
            (|scenario: &'static str| {
                let rows: Vec<_> = (1..=120)
                    .map(|number| {
                        row(
                            number,
                            format!("row-{number:04} {} 中文🙂  ", "\\\"".repeat(200)),
                        )
                    })
                    .collect();
                let mut offset = 0;
                let mut shown = Vec::new();
                loop {
                    let end = (offset + 50).min(rows.len());
                    let page = SearchResults {
                        matches: rows[offset..end].to_vec(),
                        offset,
                        next_offset: (end < rows.len()).then_some(end),
                        scanned_files: 1,
                        truncated: end < rows.len(),
                        ..Default::default()
                    };
                    let value = original(page);
                    let rendered = project(&value, &source(&value)).unwrap();
                    assert!(
                        serde_json::to_string(&rendered).unwrap().len() <= LIMIT,
                        "{scenario}"
                    );
                    let numbers: Vec<_> = rendered
                        .lines()
                        .filter_map(|line| {
                            let number = line.split_once('\t')?.0;
                            number
                                .strip_suffix(':')
                                .or_else(|| number.strip_suffix('-'))?
                                .parse::<usize>()
                                .ok()
                        })
                        .collect();
                    assert!(!numbers.is_empty(), "{scenario}");
                    assert_eq!(numbers[0], offset + 1, "{scenario}");
                    for number in &numbers {
                        assert!(rendered.contains(&rows[number - 1].line), "{scenario}");
                    }
                    offset += numbers.len();
                    shown.extend(numbers);
                    let Some(next) = metadata(&rendered)["next_offset"].as_u64() else {
                        assert_eq!(offset, rows.len(), "{scenario}");
                        break;
                    };
                    assert_eq!(next as usize, offset, "{scenario}");
                    assert!(
                        rendered.contains(&format!("same options and offset={offset}")),
                        "{scenario}"
                    );
                }
                assert_eq!(shown, (1..=rows.len()).collect::<Vec<_>>(), "{scenario}");
            }),
        ),
        (
            "separates_line_omissions_from_incomplete_scans",
            (|scenario: &'static str| {
                for partial in [false, true] {
                    let mut line = row(2, "\\\"中文🙂".repeat(3000));
                    line.truncated = true;
                    let value = original(SearchResults {
                        matches: vec![line],
                        partial,
                        next_offset: (!partial).then_some(3),
                        offset: 2,
                        skipped: 1,
                        diagnostics: (0..100)
                            .map(|_| SearchDiagnostic {
                                path: "skipped.txt".into(),
                                message: "Diagnostic 中文🙂 ".repeat(1000),
                            })
                            .collect(),
                        ..Default::default()
                    });
                    let canonical = message(value.clone());
                    let mut outgoing = vec![canonical.clone()];
                    super::super::project(&mut outgoing);
                    let rendered = text(result(&outgoing[0]));
                    assert!(
                        serde_json::to_string(rendered).unwrap().len() <= LIMIT,
                        "{scenario}"
                    );
                    assert!(
                        rendered.contains("[preview shortened] [source line truncated]"),
                        "{scenario}"
                    );
                    assert!(
                        rendered.contains("read_tool_output with output"),
                        "{scenario}"
                    );
                    assert!(rendered.contains("read_file for source text"), "{scenario}");
                    assert_eq!(metadata(rendered)["truncated"], true, "{scenario}");
                    assert_eq!(metadata(rendered)["partial"], partial, "{scenario}");
                    assert_eq!(metadata(rendered)["output"], source(&value), "{scenario}");
                    if partial {
                        assert!(metadata(rendered)["next_offset"].is_null(), "{scenario}");
                        assert!(
                            rendered.contains("Incomplete scan: narrow path"),
                            "{scenario}"
                        );
                        assert!(!rendered.contains("Next search page"), "{scenario}");
                    } else {
                        assert_eq!(metadata(rendered)["next_offset"], 3, "{scenario}");
                    }
                    assert_eq!(
                        result(&canonical).structured_content,
                        Some(value),
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "serializes_one_body",
            (|scenario: &'static str| {
                let value = original(SearchResults {
                    matches: vec![row(1, "MATCH_BODY_SENTINEL")],
                    ..Default::default()
                });
                let mut messages = vec![
                    Message::assistant().with_tool_request(
                        "search",
                        Ok(rmcp::model::CallToolRequestParams::new("search_files")),
                    ),
                    message(value),
                ];
                super::super::project(&mut messages);
                let encoded = serde_json::to_string(result(&messages[1])).unwrap();
                assert_eq!(
                    encoded.matches("MATCH_BODY_SENTINEL").count(),
                    1,
                    "{scenario}"
                );
                assert!(!encoded.contains("\\\"matches\\\":"), "{scenario}");
                assert!(!encoded.contains("structuredContent"), "{scenario}");
            }),
        ),
        (
            "preserves_five_hundred_unicode_characters",
            (|scenario: &'static str| {
                let line = "中🙂".repeat(300);
                let value = original(SearchResults {
                    matches: vec![row(1, line.clone())],
                    ..Default::default()
                });
                let rendered = project(&value, &source(&value)).unwrap();
                let expected = line.chars().take(500).collect::<String>();
                assert!(
                    rendered.contains(&format!("{expected} [preview shortened]")),
                    "{scenario}"
                );
                assert!(!rendered.contains(&line), "{scenario}");
                assert!(metadata(&rendered)["next_offset"].is_null(), "{scenario}");
                assert_eq!(value["data"]["matches"][0]["line"], line, "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
