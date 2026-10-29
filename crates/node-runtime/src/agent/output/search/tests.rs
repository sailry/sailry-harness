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
    message.content[0]
        .as_tool_response()
        .unwrap()
        .tool_result
        .as_ref()
        .unwrap()
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
        matched: number % 2 == 0,
        truncated: false,
    }
}

#[test]
fn preserves_modes_and_exact_source() {
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
        assert!(projected.structured_content.is_none());
        let rendered = text(projected);
        assert_eq!(metadata(rendered)["output"], source(&value));
        assert_eq!(metadata(rendered)["partial"], false);
        assert!(metadata(rendered)["next_offset"].is_null());
        match page.mode {
            SearchMode::Content => {
                assert!(rendered.contains("src/文件🙂.rs\n1-\t\n2:\t  needle\t中文🙂  \n"));
                assert_eq!(rendered.matches("src/文件🙂.rs").count(), 1);
            }
            SearchMode::Files => {
                assert!(rendered.contains("src/with space/文件🙂.rs\n\"line\\nbreak.rs\"\n"));
            }
            SearchMode::Count => assert!(rendered.contains("9\tsrc/文件🙂.rs\n")),
        }
        assert_eq!(result(&canonical[0]).structured_content, Some(value));
        let prepared = outgoing.clone();
        super::super::project(&mut outgoing);
        assert_eq!(outgoing, prepared);
        let mut reloaded = canonical;
        super::super::project(&mut reloaded);
        assert_eq!(reloaded, prepared);
    }
}

#[test]
fn pages_from_the_last_shown_row() {
    let rows: Vec<_> = (1..=120)
        .map(|number| {
            row(
                number,
                format!("row-{number:04} {} 中文🙂  ", "\\\"".repeat(300)),
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
        assert!(serde_json::to_string(&rendered).unwrap().len() <= LIMIT);
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
        assert!(!numbers.is_empty());
        assert_eq!(numbers[0], offset + 1);
        for number in &numbers {
            assert!(rendered.contains(&rows[number - 1].line));
        }
        offset += numbers.len();
        shown.extend(numbers);
        let Some(next) = metadata(&rendered)["next_offset"].as_u64() else {
            assert_eq!(offset, rows.len());
            break;
        };
        assert_eq!(next as usize, offset);
        assert!(rendered.contains(&format!("same options and offset={offset}")));
    }
    assert_eq!(shown, (1..=rows.len()).collect::<Vec<_>>());
}

#[test]
fn separates_line_omissions_from_incomplete_scans() {
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
        assert!(serde_json::to_string(rendered).unwrap().len() <= LIMIT);
        assert!(rendered.contains("[preview shortened] [source line truncated]"));
        assert!(rendered.contains("read_tool_output with output"));
        assert!(rendered.contains("read_file for source text"));
        assert_eq!(metadata(rendered)["truncated"], true);
        assert_eq!(metadata(rendered)["partial"], partial);
        assert_eq!(metadata(rendered)["output"], source(&value));
        if partial {
            assert!(metadata(rendered)["next_offset"].is_null());
            assert!(rendered.contains("Incomplete scan: narrow path"));
            assert!(!rendered.contains("Next search page"));
        } else {
            assert_eq!(metadata(rendered)["next_offset"], 3);
        }
        assert_eq!(result(&canonical).structured_content, Some(value));
    }
}

#[test]
fn sends_one_body_through_native_formats() {
    use goose_provider_types::{
        formats::{anthropic, google, openai, openai_responses},
        images::ImageFormat,
        model::ModelConfig,
    };
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
    let config = ModelConfig::new("fixture-search").with_max_tokens(Some(8192));
    for wire in [
        openai::create_request(&config, "", &messages, &[], &ImageFormat::OpenAi, true).unwrap(),
        openai_responses::create_responses_request(&config, "", &messages, &[]).unwrap(),
        anthropic::create_request("anthropic", &config, "", &messages, &[], Default::default())
            .unwrap(),
        google::create_request(&config, "", &messages, &[]).unwrap(),
        serde_json::to_value(result(&messages[1])).unwrap(),
    ] {
        let encoded = wire.to_string();
        assert_eq!(encoded.matches("MATCH_BODY_SENTINEL").count(), 1);
        assert!(!encoded.contains("\\\"matches\\\":"));
        assert!(!encoded.contains("structuredContent"));
    }
}
