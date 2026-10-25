use super::*;
use std::time::Duration;

#[tokio::test]
async fn returns_receipts_before_indexing() {
    let fixture = Fixture::empty().await;
    std::fs::write(fixture.root.join("main.rs"), "pub fn admitted() {}\n").unwrap();
    for client in fixture.clients() {
        let (entered, release) = fixture.node.hold_code_query_fixture();
        let request = client.prepare(Command::SearchCode {
            worktree: fixture.worktree,
            query: code_index::Search::default(),
        });
        let id = request.id;
        let mut admission = tokio::time::timeout(Duration::from_secs(5), client.dispatch(request))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(admission.receipt.id, id);
        assert!(!admission.receipt.durable);
        tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            admission.completion.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        release.cancel();
        let Output::CodeSearch(page) = admission.completion.await.unwrap().unwrap() else {
            panic!("code search expected")
        };
        assert!(page.symbols.iter().any(|symbol| symbol.name == "admitted"));
    }
    fixture.close().await;
}

#[tokio::test]
async fn cancels_admitted_queries() {
    let fixture = Fixture::empty().await;
    std::fs::write(fixture.root.join("main.rs"), "pub fn retained() {}\n").unwrap();
    for client in fixture.clients() {
        let (entered, release) = fixture.node.hold_code_query_fixture();
        let request = client.prepare(Command::SearchCode {
            worktree: fixture.worktree,
            query: code_index::Search::default(),
        });
        let id = request.id;
        let admission = client.dispatch(request).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        client
            .execute(client.prepare(Command::CancelCodeIndexQuery { request: id }))
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), admission.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert!(!database(&fixture).exists());
        release.cancel();
    }
    let [local, _] = fixture.clients();
    assert!(
        local
            .search_code(fixture.worktree, code_index::Search::default())
            .await
            .unwrap()
            .symbols
            .iter()
            .any(|symbol| symbol.name == "retained")
    );
    fixture.close().await;
}

#[tokio::test]
async fn shutdown_cancels_admitted_queries() {
    let fixture = Fixture::empty().await;
    let [local, _] = fixture.clients();
    let (entered, release) = fixture.node.hold_code_query_fixture();
    let admission = local
        .dispatch(local.prepare(Command::SearchCode {
            worktree: fixture.worktree,
            query: code_index::Search::default(),
        }))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    fixture.controller.close().await.unwrap();
    tokio::time::timeout(Duration::from_secs(30), fixture.node.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        admission.completion.await.unwrap().unwrap_err().code,
        ErrorCode::Cancelled
    );
    release.cancel();
}

#[tokio::test]
async fn opens_lazily_and_shares_cached_revisions() {
    let fixture = Fixture::empty().await;
    let first = "pub fn first_symbol() {}\n";
    std::fs::write(fixture.root.join("first.rs"), first).unwrap();
    std::fs::write(
        fixture.root.join("second.rs"),
        "pub fn second_symbol() {}\n",
    )
    .unwrap();
    assert!(!database(&fixture).exists());
    let [local, remote] = fixture.clients();
    let outline = local
        .outline_code(fixture.worktree, "first.rs")
        .await
        .unwrap();
    assert!(database(&fixture).is_file());
    assert_eq!(outline.status, code_index::Status::Parsed);
    assert_eq!(
        outline.file_revision.as_deref(),
        Some(revision(first).as_str())
    );
    assert!(
        outline
            .symbols
            .iter()
            .any(|item| item.name == "first_symbol")
    );
    assert!(outline.symbols.iter().all(|item| item.path == "first.rs"));
    let repeated = remote
        .outline_code(fixture.worktree, "first.rs")
        .await
        .unwrap();
    assert_eq!(repeated.index_revision, outline.index_revision);
    assert_eq!(repeated.symbols, outline.symbols);
    let page = remote
        .search_code(fixture.worktree, code_index::Search::default())
        .await
        .unwrap();
    assert!(page.index_revision > outline.index_revision);
    assert!(page.symbols.iter().any(|item| item.path == "first.rs"));
    assert!(page.symbols.iter().any(|item| item.path == "second.rs"));
    let unchanged = local
        .search_code(fixture.worktree, code_index::Search::default())
        .await
        .unwrap();
    assert_eq!(unchanged.index_revision, page.index_revision);
    assert_eq!(unchanged.symbols, page.symbols);
    fixture.close().await;
}

#[tokio::test]
async fn reuses_persisted_cache_after_restart() {
    let fixture = Fixture::empty().await;
    std::fs::write(
        fixture.root.join("persisted.rs"),
        "pub fn persisted_symbol() {}\n",
    )
    .unwrap();
    let before = {
        let [_, remote] = fixture.clients();
        remote
            .search_code(fixture.worktree, code_index::Search::default())
            .await
            .unwrap()
    };
    assert!(
        before
            .symbols
            .iter()
            .any(|item| item.name == "persisted_symbol")
    );
    let profile = fixture.node.profile().to_path_buf();
    let cache = database(&fixture);
    let Fixture {
        _temp,
        node,
        controller,
        worktree,
        ..
    } = fixture;
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    assert!(cache.is_file());
    let node = sailry_node_runtime::Node::start_empty(profile)
        .await
        .unwrap();
    let client = Client::new(node.local());
    let resumed = client
        .search_code(worktree, code_index::Search::default())
        .await
        .unwrap();
    assert_eq!(resumed.index_revision, before.index_revision);
    assert_eq!(resumed.symbols, before.symbols);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn filters_full_text_and_bounds_results() {
    let fixture = Fixture::empty().await;
    std::fs::create_dir(fixture.root.join("src")).unwrap();
    std::fs::create_dir(fixture.root.join("web")).unwrap();
    for (path, source) in [
        ("src/alpha.rs", "pub fn alpha_lookup() {}\n"),
        ("src/beta.rs", "pub fn beta_lookup() {}\n"),
        ("web/alpha.ts", "export function alpha_lookup() {}\n"),
    ] {
        std::fs::write(fixture.root.join(path), source).unwrap();
    }
    for client in fixture.clients() {
        let page = client
            .search_code(
                fixture.worktree,
                code_index::Search {
                    query: "alpha_lookup".into(),
                    name: Some("alpha_lookup".into()),
                    path: Some("src".into()),
                    language: Some("rust".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(!page.symbols.is_empty());
        assert!(page.symbols.iter().all(|item| {
            item.name == "alpha_lookup" && item.path == "src/alpha.rs" && item.language == "rust"
        }));
        let bounded = client
            .search_code(
                fixture.worktree,
                code_index::Search {
                    limit: 1,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(bounded.symbols.len(), 1);
        assert!(bounded.truncated);
    }
    fixture.close().await;
}

#[tokio::test]
async fn confines_paths_to_the_registered_worktree() {
    let fixture = Fixture::empty().await;
    std::fs::write(
        fixture._temp.path().join("outside.rs"),
        "pub fn outside_symbol() {}\n",
    )
    .unwrap();
    for client in fixture.clients() {
        assert_eq!(
            client
                .outline_code(fixture.worktree, "../outside.rs")
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client
                .search_code(
                    fixture.worktree,
                    code_index::Search {
                        path: Some("../".into()),
                        ..Default::default()
                    },
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    assert!(!database(&fixture).exists());
    fixture.close().await;
}
