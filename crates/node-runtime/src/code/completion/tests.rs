use super::*;
use crate::code::{operations, tests::Fixture};
use sailry_link::CancellationToken;
use serde_json::json;

#[tokio::test]
async fn returns_only_confined_plain_main_edits() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"completions":{"isIncomplete":true,"items":[
        {"label":"valid","textEdit":{"range":{"start":{"line":0,"character":3},"end":{"line":0,"character":9}},"newText":"valid"}},
        {"label":"snippet","insertText":"${1:unsafe}","insertTextFormat":2},
        {"label":"command","command":{"command":"modify"}},
        {"label":"extra","additionalTextEdits":[{"newText":"modify"}]},
        {"label":"split","textEdit":{"range":{"start":{"line":0,"character":1},"end":{"line":0,"character":2}},"newText":"invalid"}}
    ]}}));
    let stop = CancellationToken::new();
    let server = Server::start(
        fixture.plan(),
        &fixture.environment,
        CancellationToken::new(),
        &stop,
    )
    .await
    .unwrap();
    let result = operations::read(
        &server,
        &code::Query::Completion {
            path: "main.rs".into(),
            position: code::Position { line: 1, column: 3 },
        },
        None,
        &stop,
    )
    .await
    .unwrap();
    assert!(result.partial);
    assert_eq!(
        result.revision.as_deref(),
        Some(
            blake3::hash("🙂 broken symbol\n".as_bytes())
                .to_hex()
                .to_string()
                .as_str()
        )
    );
    let code::Content::Completions(items) = result.content else {
        panic!("completions expected")
    };
    assert_eq!(items.len(), 1);
    let range = items[0].range.as_ref().unwrap();
    assert_eq!(range.path, "main.rs");
    assert_eq!(range.start.column, 3);
    assert_eq!(range.end.column, 9);
    assert_eq!(items[0].insert_text, "valid");
    server.rpc.shutdown().await;
}

#[tokio::test]
async fn binds_positions_and_results_to_draft_revision() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"completions":[{"label":"fixture","insertText":"value"}]}));
    let stop = CancellationToken::new();
    let server = Server::start(
        fixture.plan(),
        &fixture.environment,
        CancellationToken::new(),
        &stop,
    )
    .await
    .unwrap();
    let text = "中文🙂 symbol\n";
    let saved = crate::code::documents::read(&fixture.root, "main.rs").unwrap();
    let overlay = code::Overlay {
        path: "main.rs".into(),
        expected_revision: saved.revision,
        text: text.into(),
    };
    let query = code::Query::Completion {
        path: "main.rs".into(),
        position: code::Position { line: 1, column: 5 },
    };
    let result = operations::read(&server, &query, Some(&overlay), &stop)
        .await
        .unwrap();
    assert_eq!(
        result.revision.unwrap(),
        blake3::hash(text.as_bytes()).to_hex().to_string()
    );
    let messages = fixture.messages();
    let request = messages
        .iter()
        .find(|message| message["method"] == "textDocument/completion")
        .unwrap();
    assert_eq!(request["params"]["position"]["character"], 5);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("main.rs")).unwrap(),
        saved.text
    );
    let query = code::Query::Completion {
        path: "main.rs".into(),
        position: code::Position {
            line: 1,
            column: 999,
        },
    };
    assert!(
        operations::read(&server, &query, Some(&overlay), &stop)
            .await
            .is_err()
    );
    server.rpc.shutdown().await;
}
