#![cfg(unix)]
use super::*;
use crate::code::{server::Server, tests::Fixture};
#[tokio::test]
async fn synchronizes_external_changes_and_confines_locations() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"outside":true,"hover_size":10000}));
    let stop = CancellationToken::new();
    let server = Server::start(
        fixture.plan(),
        &fixture.environment,
        CancellationToken::new(),
        &stop,
    )
    .await
    .unwrap();
    let query = Query::Diagnostics {
        path: "main.rs".into(),
        severity: None,
    };
    let result = read(&server, &query, None, &stop).await.unwrap();
    let Content::Diagnostics(items) = result.content else {
        panic!("diagnostics expected")
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].location.start.column, 3);
    std::fs::write(fixture.root.join("main.rs"), "symbol is valid\n").unwrap();
    let Content::Diagnostics(items) = read(&server, &query, None, &stop).await.unwrap().content
    else {
        panic!("diagnostics expected")
    };
    assert!(items.is_empty());
    let Content::Locations(items) = read(
        &server,
        &Query::Definition {
            path: "main.rs".into(),
            line: 1,
            symbol: "symbol".into(),
        },
        None,
        &stop,
    )
    .await
    .unwrap()
    .content
    else {
        panic!("locations expected")
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].path, "target.rs");
    assert_eq!(items[0].start.line, 1);
    let result = read(
        &server,
        &Query::Hover {
            path: "main.rs".into(),
            line: 1,
            symbol: "symbol".into(),
        },
        None,
        &stop,
    )
    .await
    .unwrap();
    assert!(result.partial);
    let Content::Hover(text) = result.content else {
        panic!("hover expected")
    };
    assert!(text.len() <= 8192);
    assert!(text.ends_with('🙂'));
    let messages = fixture.messages();
    assert!(
        messages
            .iter()
            .any(|message| message["method"] == "textDocument/didChange")
    );
    assert!(
        messages
            .iter()
            .any(|message| message["id"] == "edit" && message["result"]["applied"] == false)
    );
    server.rpc.shutdown().await;
}
#[tokio::test]
async fn keeps_overlays_off_disk_and_rejects_conflicts() {
    let fixture = Fixture::new().await;
    let stop = CancellationToken::new();
    let server = Server::start(
        fixture.plan(),
        &fixture.environment,
        CancellationToken::new(),
        &stop,
    )
    .await
    .unwrap();
    let saved = documents::read(&fixture.root, "main.rs").unwrap();
    let overlay = code::Overlay {
        path: "main.rs".into(),
        expected_revision: saved.revision,
        text: "symbol is valid\n".into(),
    };
    let result = read(
        &server,
        &Query::Diagnostics {
            path: "main.rs".into(),
            severity: None,
        },
        Some(&overlay),
        &stop,
    )
    .await
    .unwrap();
    let Content::Diagnostics(items) = result.content else {
        panic!("diagnostics expected")
    };
    assert!(items.is_empty());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("main.rs")).unwrap(),
        saved.text
    );
    std::fs::write(fixture.root.join("main.rs"), "external change\n").unwrap();
    assert_eq!(
        read(
            &server,
            &Query::Diagnostics {
                path: "main.rs".into(),
                severity: None
            },
            Some(&overlay),
            &stop
        )
        .await
        .unwrap_err()
        .code,
        sailry_protocol::ErrorCode::RevisionConflict
    );
    server.rpc.shutdown().await;
}
#[tokio::test]
async fn marks_indexing_results_as_partial() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"indexing":true}));
    let stop = CancellationToken::new();
    let server = Server::start(
        fixture.plan(),
        &fixture.environment,
        CancellationToken::new(),
        &stop,
    )
    .await
    .unwrap();
    let result = read(
        &server,
        &Query::Hover {
            path: "main.rs".into(),
            line: 1,
            symbol: "symbol".into(),
        },
        None,
        &stop,
    )
    .await
    .unwrap();
    assert_eq!(result.status, Status::Indexing);
    assert!(result.partial);
    assert!(result.notice.unwrap().contains("indexing"));
    server.rpc.shutdown().await;
}
