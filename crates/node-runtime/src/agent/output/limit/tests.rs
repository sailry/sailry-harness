use super::*;
use crate::agent::canonical::BlockKind;
use crate::agent::engine::history;
use crate::agent::model::types::ToolResult;
use rmcp::model::CallToolResult;
use std::collections::BTreeMap;

fn message(result: CallToolResult) -> Message {
    Message::user()
        .with_generated_id()
        .with_content(Content::ToolResult(ToolResult {
            id: "call".into(),
            name: "plugin_tool".into(),
            result: Ok(result),
            metadata: None,
        }))
}

fn result(message: &Message) -> &CallToolResult {
    let Content::ToolResult(response) = &message.content[0] else {
        panic!("tool response expected")
    };
    response.result.as_ref().unwrap()
}

fn texts(result: &CallToolResult) -> Vec<&str> {
    result.content.iter().filter_map(text).collect()
}

fn projected(original: &Message) -> Message {
    let mut outgoing = vec![original.clone()];
    project(&mut outgoing);
    outgoing.pop().unwrap()
}

/// The notice's reference JSON and continuation offset.
fn reference_of(notice: &str) -> (Value, usize) {
    let (_, rest) = notice
        .split_once("output=")
        .expect("notice has a reference");
    let (output, rest) = rest.split_once(" and offset=").unwrap();
    let (offset, _) = rest.split_once(',').unwrap();
    (
        serde_json::from_str(output).unwrap(),
        offset.parse().unwrap(),
    )
}

async fn database() -> (turso::Database, turso::Connection) {
    let database = turso::Builder::new_local(":memory:").build().await.unwrap();
    let db = database.connect().unwrap();
    db.execute(
        "CREATE TABLE agent_blobs(digest TEXT PRIMARY KEY,bytes BLOB NOT NULL)",
        (),
    )
    .await
    .unwrap();
    (database, db)
}

/// Every page of the canonical JSON, as `read_tool_output` serves them.
fn pages(canonical: &str, mut offset: usize) -> String {
    let mut combined = String::new();
    loop {
        let range = crate::process::output::utf8_page(
            canonical.as_bytes(),
            offset,
            sailry_protocol::tool::MAX_OUTPUT_PAGE_BYTES,
        )
        .unwrap();
        combined.push_str(&canonical[range.clone()]);
        if range.end == canonical.len() {
            return combined;
        }
        offset = range.end;
    }
}

#[test]
fn preserves_result_projection() {
    for (scenario, check) in [
        (
            "leaves_small_results_unchanged",
            (|scenario: &'static str| {
                for original in [
                    message(CallToolResult::success(vec![ContentBlock::text(
                        "line\n".repeat(MAX_LINES),
                    )])),
                    message(CallToolResult::structured(json!({"text":"中文🙂"}))),
                    message(CallToolResult::success(vec![ContentBlock::text(
                        "x".repeat(MAX_BYTES),
                    )])),
                ] {
                    assert_eq!(projected(&original), original, "{scenario}");
                }
            }) as fn(&'static str),
        ),
        (
            "bounds_bytes_before_lines",
            (|scenario: &'static str| {
                let raw = "x".repeat(200 * 1024);
                let original = message(CallToolResult::success(vec![ContentBlock::text(&raw)]));
                let outgoing = projected(&original);
                let shown = texts(result(&outgoing));
                assert_eq!(shown.len(), 2, "{scenario}");
                assert!(shown[0].len() > MAX_BYTES - 1024, "{scenario}");
                assert!(raw.starts_with(shown[0]), "{scenario}");
                assert!(
                    shown.iter().map(|text| text.len()).sum::<usize>() <= MAX_BYTES,
                    "{scenario}"
                );
                assert!(
                    shown[1].contains(&format!(
                        "showing the first 1 of 1 lines ({} of {} bytes)",
                        shown[0].len(),
                        raw.len()
                    )),
                    "{scenario}"
                );
                // Plain MCP text pages the whole serialized result from its start.
                assert_eq!(reference_of(shown[1]).1, 0, "{scenario}");
                assert_eq!(
                    result(&original).content[0],
                    ContentBlock::text(&raw),
                    "{scenario}"
                );
            }),
        ),
        (
            "bounds_lines_before_bytes",
            (|scenario: &'static str| {
                let raw: String = (0..5_000).map(|number| format!("{number}\n")).collect();
                assert!(raw.len() < MAX_BYTES, "{scenario}");
                let original = message(CallToolResult::success(vec![ContentBlock::text(&raw)]));
                let outgoing = projected(&original);
                let shown = texts(result(&outgoing));
                assert_eq!(count(shown[0]), MAX_LINES - 1, "{scenario}");
                assert!(
                    shown[0].ends_with(&format!("{}\n", MAX_LINES - 1 - 1)),
                    "{scenario}"
                );
                assert!(
                    shown[1].contains(&format!(
                        "showing the first {} of 5000 lines",
                        MAX_LINES - 1
                    )),
                    "{scenario}"
                );
                assert!(
                    shown.iter().map(|text| count(text)).sum::<usize>() <= MAX_LINES,
                    "{scenario}"
                );
            }),
        ),
        (
            "keeps_whole_lines_and_utf8_characters",
            (|scenario: &'static str| {
                let line = format!("{}\n", "中文🙂".repeat(100));
                let raw = line.repeat(200);
                let outgoing =
                    projected(&message(CallToolResult::success(vec![ContentBlock::text(
                        &raw,
                    )])));
                let shown = texts(result(&outgoing))[0];
                assert!(raw.starts_with(shown), "{scenario}");
                assert!(shown.ends_with('\n'), "{scenario}");
                assert_eq!(shown.len() % line.len(), 0, "{scenario}");

                let raw = "中文🙂".repeat(20 * 1024);
                let outgoing =
                    projected(&message(CallToolResult::success(vec![ContentBlock::text(
                        &raw,
                    )])));
                let shown = texts(result(&outgoing))[0];
                assert!(raw.starts_with(shown), "{scenario}");
                assert!(shown.len() <= MAX_BYTES, "{scenario}");
                assert!(shown.len() > MAX_BYTES - 1024, "{scenario}");
            }),
        ),
        (
            "keeps_media_and_hides_later_text",
            (|scenario: &'static str| {
                let image = ContentBlock::image("AQID", "image/png");
                let original = message(CallToolResult::success(vec![
                    ContentBlock::text("a".repeat(60 * 1024)),
                    image.clone(),
                    ContentBlock::text("later"),
                ]));
                let outgoing = projected(&original);
                let content = &result(&outgoing).content;
                assert_eq!(content.len(), 3, "{scenario}");
                assert!(
                    matches!(&content[0], ContentBlock::Text(text) if text.text.len() < MAX_BYTES),
                    "{scenario}"
                );
                assert_eq!(content[1], image, "{scenario}");
                assert!(
                    texts(result(&outgoing))[1].starts_with("[Tool result truncated"),
                    "{scenario}"
                );
            }),
        ),
        (
            "repeats_without_change",
            (|scenario: &'static str| {
                let original = message(CallToolResult::structured(
                    json!({"rows":vec!["中文🙂 row"; 20_000]}),
                ));
                let once = projected(&original);
                assert_eq!(projected(&once), once, "{scenario}");
            }),
        ),
        (
            "bounds_structured_only_and_extra_blocks",
            (|scenario: &'static str| {
                let value = json!({"text":"x".repeat(MAX_BYTES * 3)});
                let mut structured = CallToolResult::success(Vec::new());
                structured.structured_content = Some(value);
                let mut file = CallToolResult::structured(json!({"kind":"file_lines","data":{
                "path":"file.txt","lines":[{"number":1,"text":"content\n"}],
                "size":8,"revision":"a".repeat(64),"offset":0,"next_offset":null,
                    }}));
                file.content
                    .push(ContentBlock::text("extra\n".repeat(MAX_LINES * 3)));
                let resource = CallToolResult::success(vec![ContentBlock::resource(
                    ResourceContents::text("resource\n".repeat(MAX_LINES * 3), "fixture://source"),
                )]);
                for source in [structured, file, resource] {
                    let original = message(source);
                    let outgoing = projected(&original);
                    let (bytes, lines) = size(result(&outgoing));
                    assert!(bytes <= MAX_BYTES && lines <= MAX_LINES, "{scenario}");
                    assert!(result(&outgoing).structured_content.is_none(), "{scenario}");
                    assert_eq!(projected(&outgoing), outgoing, "{scenario}");
                    let text = texts(result(&outgoing));
                    assert!(
                        text.last().unwrap().contains("read_tool_output"),
                        "{scenario}"
                    );
                    assert_eq!(reference_of(text.last().unwrap()).1, 0, "{scenario}");
                }
            }),
        ),
        (
            "budgets_long_call_references",
            (|scenario: &'static str| {
                let mut original = message(CallToolResult::success(vec![ContentBlock::text(
                    "x".repeat(MAX_BYTES * 2),
                )]));
                let Content::ToolResult(response) = &mut original.content[0] else {
                    unreachable!()
                };
                response.id = "call".repeat(1024);
                let outgoing = projected(&original);
                assert!(size(result(&outgoing)).0 <= MAX_BYTES, "{scenario}");
                assert_eq!(projected(&outgoing), outgoing, "{scenario}");
            }),
        ),
        (
            "bounds_blank_line_tails",
            (|scenario: &'static str| {
                for text in [
                    "\n".repeat(5000),
                    format!("first\n{}last\n", "x\n".repeat(5000)),
                ] {
                    let start = tail_start(&text, MAX_BYTES, 100);
                    assert_eq!(count(&text[start..]), 100, "{scenario}");
                    assert!(text.ends_with(&text[start..]), "{scenario}");
                }
            }),
        ),
        (
            "renders_raw_pages_without_nested_escaping",
            (|scenario: &'static str| {
                let raw = "\\\"".repeat(10000);
                let page = sailry_protocol::tool::OutputPage {
                    turn: sailry_protocol::TurnId::new(),
                    output: sailry_protocol::tool::OutputReference {
                        call: "source".into(),
                        revision: "a".repeat(64),
                    },
                    offset: 42,
                    total_bytes: 100000,
                    next_offset: Some(42 + raw.len()),
                    text: raw.clone(),
                };
                let original = message(CallToolResult::structured(
                    serde_json::to_value(&page).unwrap(),
                ));
                let outgoing = projected(&original);
                let contents = texts(result(&outgoing));
                assert_eq!(contents.len(), 1, "{scenario}");
                assert!(contents[0].contains(&raw), "{scenario}");
                assert!(!contents[0].contains("Tool result truncated"), "{scenario}");
                let metadata: Value =
                    serde_json::from_str(contents[0].lines().last().unwrap()).unwrap();
                assert_eq!(
                    metadata["output"],
                    serde_json::to_value(&page.output).unwrap(),
                    "{scenario}"
                );
                assert_eq!(metadata["next_offset"], 42 + raw.len(), "{scenario}");
                assert!(size(result(&outgoing)).0 <= MAX_BYTES, "{scenario}");
            }),
        ),
        (
            "bounds_error_data_without_changing_status",
            (|scenario: &'static str| {
                let error = rmcp::model::ErrorData::internal_error(
                    "Failure headline",
                    Some(json!({"detail":"中文🙂".repeat(MAX_BYTES)})),
                );
                let original = Message::user().with_tool_response("failure", Err(error.clone()));
                let outgoing = projected(&original);
                let Content::ToolResult(response) = &outgoing.content[0] else {
                    panic!("tool response expected")
                };
                let failure = response.result.as_ref().unwrap_err();
                assert_eq!(failure.code, error.code, "{scenario}");
                assert!(
                    failure.message.starts_with("Failure headline"),
                    "{scenario}"
                );
                assert!(failure.message.len() <= MAX_BYTES, "{scenario}");
                assert!(failure.data.is_none(), "{scenario}");
                let (reference, offset) = reference_of(&failure.message);
                assert_eq!(offset, 0, "{scenario}");
                assert_eq!(
                    reference["revision"],
                    blake3::hash(serde_json::to_value(&error).unwrap().to_string().as_bytes())
                        .to_hex()
                        .to_string(),
                    "{scenario}"
                );
                assert_eq!(projected(&outgoing), outgoing, "{scenario}");
                let Content::ToolResult(response) = &original.content[0] else {
                    panic!("tool response expected")
                };
                assert_eq!(response.result.as_ref().unwrap_err(), &error, "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

mod canonical {
    use super::*;

    async fn stored(db: &turso::Connection, original: &Message) -> (String, String) {
        let record = history::capture(
            db,
            sailry_protocol::TurnId::new(),
            original,
            &BTreeMap::from([("call".into(), "plugin_tool".into())]),
        )
        .await
        .unwrap();
        let BlockKind::ToolResponse(response) = &record.message().unwrap().content[0].kind else {
            panic!("tool response expected")
        };
        let value = crate::store::agent::blobs::hydrate(db, &response.result)
            .await
            .unwrap();
        // The reloaded copy projects to the same notice as the live copy.
        let reloaded = history::project(db, &record).await.unwrap().unwrap();
        assert_eq!(projected(&reloaded).content, projected(original).content);
        (response.revision.clone(), value.to_string())
    }

    #[tokio::test]
    async fn pages_mirrored_json_after_preview() {
        let (_database, db) = database().await;
        let original = message(CallToolResult::structured(
            json!({"rows":vec!["中文🙂 row"; 20_000]}),
        ));
        let (revision, canonical) = stored(&db, &original).await;
        let outgoing = projected(&original);
        assert!(result(&outgoing).structured_content.is_none());
        let shown = texts(result(&outgoing));
        let (output, offset) = reference_of(shown[1]);
        assert_eq!(output, json!({"call":"call","revision":revision}));
        assert_eq!(offset, shown[0].len());
        assert!(canonical.starts_with(shown[0]));
        assert_eq!(
            format!("{}{}", shown[0], pages(&canonical, offset)),
            canonical
        );
        assert_eq!(projected(&original), outgoing);
        assert_eq!(
            result(&original)
                .structured_content
                .as_ref()
                .unwrap()
                .to_string(),
            canonical
        );
    }

    #[tokio::test]
    async fn pages_whole_mcp_result() {
        let (_database, db) = database().await;
        let raw = "error 中文🙂\n".repeat(10_000);
        let mut source = CallToolResult::success(vec![
            ContentBlock::text(&raw),
            ContentBlock::image("AQID", "image/png"),
        ]);
        source.structured_content = Some(json!({"lines":10_000}));
        source.meta = Some(serde_json::from_value(json!({"opaque":"kept"})).unwrap());
        let original = message(source);
        let (revision, canonical) = stored(&db, &original).await;
        let outgoing = projected(&original);
        let shown = texts(result(&outgoing));
        let (output, offset) = reference_of(shown[1]);
        assert_eq!(output, json!({"call":"call","revision":revision}));
        assert_eq!(offset, 0);
        let restored: CallToolResult = serde_json::from_str(&pages(&canonical, 0)).unwrap();
        assert_eq!(&restored, result(&original));
    }
}

#[tokio::test]
async fn references_canonical_data_after_media_projection() {
    use crate::agent::canonical::Kind;
    let (_database, db) = database().await;
    let value = json!({"text":"source\n".repeat(MAX_LINES * 5),
        "inline_data":[{"mime_type":"image/png","data":"AQID"}]});
    let original = message(CallToolResult::structured(value.clone()));
    let record = history::capture(
        &db,
        sailry_protocol::TurnId::new(),
        &original,
        &BTreeMap::new(),
    )
    .await
    .unwrap();
    let reloaded = history::project(&db, &record).await.unwrap().unwrap();
    assert!(
        result(&reloaded)
            .content
            .iter()
            .any(|content| matches!(content, ContentBlock::Image(_)))
    );
    assert!(
        result(&reloaded)
            .structured_content
            .as_ref()
            .unwrap()
            .get("inline_data")
            .is_none()
    );
    let outgoing = projected(&reloaded);
    let texts = texts(result(&outgoing));
    let (reference, offset) = reference_of(texts.last().unwrap());
    assert_eq!(offset, 0);
    assert_eq!(
        reference["revision"],
        blake3::hash(value.to_string().as_bytes())
            .to_hex()
            .to_string()
    );
    assert!(
        result(&outgoing)
            .content
            .iter()
            .any(|content| matches!(content, ContentBlock::Image(_)))
    );
    let captured = history::capture(&db, record.turn, &reloaded, &BTreeMap::new())
        .await
        .unwrap();
    assert!(captured.metadata.runtime.as_ref().unwrap().notes.is_none());
    assert!(matches!(captured.kind, Kind::Message(_)));
}
