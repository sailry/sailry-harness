use super::*;

fn diagnostics(path: &str) -> Query {
    Query::Diagnostics {
        path: path.into(),
        severity: None,
    }
}

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

fn changes(fixture: &Fixture) -> Vec<Value> {
    std::fs::read_to_string(fixture.root.join("messages"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|message| message["method"] == "textDocument/didChange")
        .collect()
}

async fn start(fixture: &Fixture) {
    let result = fixture.read(diagnostics("main.rs"), None).await;
    assert_eq!(result.status, code::Status::Stopped);
    assert!(!fixture.root.join("pid").exists());
    let output = fixture
        .client
        .execute(fixture.client.prepare(Command::StartCodeServer {
            startup: result.startup.unwrap(),
        }))
        .await
        .unwrap();
    assert!(matches!(output, Output::CodeServer(_)));
    let result = fixture.read(diagnostics("main.rs"), None).await;
    assert_eq!(result.revision, Some(revision("🙂 broken symbol\n")));
    assert!(matches!(result.content, Content::Diagnostics(items) if items.len() == 1));
}

#[tokio::test]
async fn retains_drafts_across_reads() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        start(&fixture).await;
        let saved = "🙂 broken symbol\n";
        let draft = "valid symbol\n";
        let overlay = code::Overlay {
            path: "main.rs".into(),
            expected_revision: revision(saved),
            text: draft.into(),
        };
        let result = fixture
            .read(diagnostics("main.rs"), Some(overlay.clone()))
            .await;
        assert_eq!(result.revision, Some(revision(draft)));
        assert!(matches!(result.content, Content::Diagnostics(items) if items.is_empty()));
        assert!(
            matches!(fixture.read(diagnostics("target.rs"), None).await.content,
            Content::Diagnostics(items) if items.is_empty())
        );
        let mut subscription = fixture
            .client
            .subscribe_code_servers(fixture.session.worktree)
            .await
            .unwrap();
        let initial = subscription.next().await.unwrap();
        assert!(
            matches!(&initial, sailry_protocol::Update::CodeServers { node, worktree, servers, .. }
            if *node == fixture.client.target() && *worktree == fixture.session.worktree && servers.len() == 1)
        );
        for _ in 0..3 {
            let result = fixture
                .read(diagnostics("main.rs"), Some(overlay.clone()))
                .await;
            assert_eq!(result.revision, Some(revision(draft)));
            assert!(matches!(result.content, Content::Diagnostics(items) if items.is_empty()));
            assert!(
                matches!(fixture.read(diagnostics("target.rs"), None).await.content,
                Content::Diagnostics(items) if items.is_empty())
            );
        }
        let sent = changes(&fixture);
        assert_eq!(sent.len(), 1, "unchanged draft: remote={remote}");
        assert_eq!(
            sent[0].pointer("/params/contentChanges/0/text"),
            Some(&json!(draft))
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(400), subscription.next())
                .await
                .is_err(),
            "unchanged diagnostics published another revision: remote={remote}"
        );
        let Output::CodeServers(current) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCodeServers {
                worktree: fixture.session.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!("servers expected")
        };
        let sailry_protocol::Update::CodeServers { servers, .. } = initial else {
            unreachable!("initial code update was checked")
        };
        assert_eq!(current, servers);
        let result = fixture.read(diagnostics("main.rs"), None).await;
        assert_eq!(result.revision, Some(revision(saved)));
        assert!(matches!(result.content, Content::Diagnostics(items) if items.len() == 1));
        let sent = changes(&fixture);
        assert_eq!(sent.len(), 2);
        assert_eq!(
            sent[1].pointer("/params/contentChanges/0/text"),
            Some(&json!(saved))
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("main.rs")).unwrap(),
            saved
        );
        drop(subscription);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_drafts_after_external_changes() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        start(&fixture).await;
        let saved = "🙂 broken symbol\n";
        let draft = "valid symbol\n";
        let overlay = code::Overlay {
            path: "main.rs".into(),
            expected_revision: revision(saved),
            text: draft.into(),
        };
        let result = fixture
            .read(diagnostics("main.rs"), Some(overlay.clone()))
            .await;
        assert_eq!(result.revision, Some(revision(draft)));
        let external = "external valid symbol\n";
        std::fs::write(fixture.root.join("main.rs"), external).unwrap();
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadCode {
                worktree: fixture.session.worktree,
                query: diagnostics("main.rs"),
                overlay: Some(overlay),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        let result = fixture.read(diagnostics("main.rs"), None).await;
        assert_eq!(result.revision, Some(revision(external)));
        assert!(matches!(result.content, Content::Diagnostics(items) if items.is_empty()));
        let updated = "broken overlay symbol\n";
        let result = fixture
            .read(
                diagnostics("main.rs"),
                Some(code::Overlay {
                    path: "main.rs".into(),
                    expected_revision: revision(external),
                    text: updated.into(),
                }),
            )
            .await;
        assert_eq!(result.revision, Some(revision(updated)));
        let Content::Diagnostics(items) = result.content else {
            panic!("diagnostics expected")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].message, "fixture syntax error");
        assert_eq!(
            items[0].location.start,
            code::Position { line: 1, column: 1 }
        );
        assert_eq!(items[0].location.end, code::Position { line: 1, column: 7 });
        let sent = changes(&fixture);
        assert_eq!(sent.len(), 3, "external resynchronization: remote={remote}");
        for (message, text) in sent.iter().zip([draft, external, updated]) {
            assert_eq!(
                message.pointer("/params/contentChanges/0/text"),
                Some(&json!(text))
            );
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("main.rs")).unwrap(),
            external
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
