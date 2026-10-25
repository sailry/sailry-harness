use super::*;
use std::{path::Path, sync::atomic::Ordering};

fn populate(fixture: &Fixture, count: usize) {
    for file in 0..count {
        fixture.write(
            &format!("file_{file:03}.rs"),
            &format!("pub fn item_{file}() {{}}\n"),
        );
    }
}

async fn advance(fixture: &Fixture, batches: usize) -> sync::Report {
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let mut progress = worktree.sync.lock().await;
    fixture
        .service
        .synchronize(
            &worktree,
            worktree.index().await.unwrap(),
            fixture.service.resources().await.unwrap(),
            &CancellationToken::new(),
            &mut progress,
            sync::Limits {
                batches,
                ..Default::default()
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn resumes_committed_batches() {
    let fixture = Fixture::new();
    populate(&fixture, BATCH + 3);
    let first = advance(&fixture, 1).await;
    assert!(first.partial && first.truncated);
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    assert_eq!(index.files().await.unwrap().len(), BATCH);
    let resources = fixture.service.resources().await.unwrap();
    assert_eq!(resources.parser.parsed.load(Ordering::Relaxed), BATCH);
    let second = advance(&fixture, 1).await;
    assert!(!second.partial && !second.truncated);
    assert_eq!(index.files().await.unwrap().len(), BATCH + 3);
    assert_eq!(resources.parser.parsed.load(Ordering::Relaxed), BATCH + 3);
    assert_eq!(
        index.outline("file_000.rs").await.unwrap().index_revision,
        2
    );
}

#[tokio::test]
async fn preserves_records_until_all_batches_finish() {
    let fixture = Fixture::new();
    fixture.write("retired.rs", "pub fn retained() {}\n");
    fixture.search().await;
    std::fs::remove_file(fixture.root.join("retired.rs")).unwrap();
    populate(&fixture, BATCH + 3);
    fixture.service.changed(fixture.id, None);
    let first = advance(&fixture, 1).await;
    assert!(first.partial && first.truncated);
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    assert!(index.files().await.unwrap().contains_key("retired.rs"));
    advance(&fixture, usize::MAX).await;
    assert!(!index.files().await.unwrap().contains_key("retired.rs"));
}

#[tokio::test]
async fn exhaustion_preserves_valid_metadata() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn preserved() {}\n");
    let first = fixture.search().await;
    fixture.write("main.rs", "pub fn refreshed() {}\n");
    fixture.service.changed(fixture.id, Some("main.rs"));
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let mut permits = Vec::new();
    for _ in 0..4 {
        permits.push(
            fixture
                .service
                .parsing
                .clone()
                .acquire_owned()
                .await
                .unwrap(),
        );
    }
    let report = fixture
        .service
        .synchronize(
            &worktree,
            worktree.index().await.unwrap(),
            fixture.service.resources().await.unwrap(),
            &CancellationToken::new(),
            &mut *worktree.sync.lock().await,
            sync::Limits {
                deadline: Instant::now() + Duration::from_millis(50),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(report.partial && report.truncated);
    let outline = worktree
        .index()
        .await
        .unwrap()
        .outline("main.rs")
        .await
        .unwrap();
    assert_eq!(outline.status, Status::Parsed);
    assert_eq!(outline.index_revision, first.index_revision);
    assert!(
        outline
            .symbols
            .iter()
            .any(|symbol| symbol.name == "preserved")
    );
    drop(permits);
    assert!(
        fixture
            .search()
            .await
            .symbols
            .iter()
            .any(|symbol| symbol.name == "refreshed")
    );
}

#[tokio::test]
async fn serves_cached_results_during_refresh() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn preserved() {}\n");
    let first = fixture.search().await;
    fixture.write("main.rs", "pub fn refreshed() {}\n");
    fixture.service.changed(fixture.id, Some("main.rs"));
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let (entered, paused) = tokio::sync::oneshot::channel();
    let (resume, wait) = tokio::sync::oneshot::channel();
    *worktree.prepared.lock().unwrap() = Some((entered, wait));
    let service = fixture.service.clone();
    let root = fixture.root.clone();
    let id = fixture.id;
    let indexing = tokio::spawn(async move {
        service
            .search(id, root, &Search::default(), &CancellationToken::new())
            .await
            .unwrap()
    });
    paused.await.unwrap();
    let resources = fixture.service.resources().await.unwrap();
    let parsed = resources.parser.parsed.load(Ordering::Relaxed);
    let page = tokio::time::timeout(Duration::from_secs(5), fixture.search())
        .await
        .unwrap();
    assert!(page.partial && page.truncated);
    assert_eq!(page.index_revision, first.index_revision);
    assert!(page.symbols.iter().any(|symbol| symbol.name == "preserved"));
    assert_eq!(resources.parser.parsed.load(Ordering::Relaxed), parsed);
    resume.send(()).unwrap();
    let fresh = indexing.await.unwrap();
    assert!(
        fresh
            .symbols
            .iter()
            .any(|symbol| symbol.name == "refreshed")
    );
}

#[tokio::test]
async fn rechecks_committed_paths_after_changes() {
    let fixture = Fixture::new();
    populate(&fixture, BATCH + 3);
    advance(&fixture, 1).await;
    fixture.write("file_000.rs", "pub fn changed() {}\n");
    fixture.service.changed(fixture.id, Some("file_000.rs"));
    let next = advance(&fixture, 1).await;
    assert!(next.partial && next.truncated);
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    let outline = index.outline("file_000.rs").await.unwrap();
    assert!(
        outline
            .symbols
            .iter()
            .any(|symbol| symbol.name == "changed")
    );
    let resources = fixture.service.resources().await.unwrap();
    assert_eq!(resources.parser.parsed.load(Ordering::Relaxed), BATCH + 4);
    advance(&fixture, 1).await;
    assert_eq!(index.files().await.unwrap().len(), BATCH + 3);
    assert_eq!(resources.parser.parsed.load(Ordering::Relaxed), BATCH + 4);
}

#[tokio::test]
async fn defers_new_dirty_paths() {
    let fixture = Fixture::new();
    fixture.write(".gitignore", "ignored.rs\n");
    populate(&fixture, BATCH + 3);
    let mut expected: BTreeSet<_> = (0..BATCH + 3)
        .map(|number| format!("file_{number:03}.rs"))
        .collect();
    expected.insert(".gitignore".into());
    advance(&fixture, 1).await;
    for path in ["ignored.rs", "new.rs"] {
        fixture.write(path, "pub fn discovered() {}\n");
        fixture.service.changed(fixture.id, Some(path));
    }
    advance(&fixture, 1).await;
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    let files = index.files().await.unwrap();
    assert_eq!(files.keys().cloned().collect::<BTreeSet<_>>(), expected);
    assert!(!files.contains_key("ignored.rs"));
    assert!(!files.contains_key("new.rs"));

    advance(&fixture, usize::MAX).await;
    let files = index.files().await.unwrap();
    expected.insert("new.rs".into());
    assert_eq!(files.keys().cloned().collect::<BTreeSet<_>>(), expected);
    assert!(!files.contains_key("ignored.rs"));
    assert!(files.contains_key("new.rs"));
}

#[tokio::test]
async fn continues_across_root_invalidations() {
    let fixture = Fixture::new();
    let count = BATCH * 2 + 3;
    populate(&fixture, count);
    advance(&fixture, 1).await;
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    let resources = fixture.service.resources().await.unwrap();
    fixture.service.changed(fixture.id, None);
    let second = advance(&fixture, 1).await;
    assert!(second.partial && second.truncated);
    assert_eq!(index.files().await.unwrap().len(), BATCH * 2);
    fixture.service.changed(fixture.id, None);
    let third = advance(&fixture, 1).await;
    assert!(third.partial && third.truncated);
    assert_eq!(index.files().await.unwrap().len(), count);
    assert_eq!(resources.parser.parsed.load(Ordering::Relaxed), count);
    let reconciled = advance(&fixture, usize::MAX).await;
    assert!(!reconciled.partial && !reconciled.truncated);
    assert_eq!(resources.parser.parsed.load(Ordering::Relaxed), count);
    let dirty = worktree.dirty.lock().unwrap();
    assert_eq!(dirty.clean, dirty.generation);
}

#[tokio::test]
async fn resumes_after_cancelling_a_segment() {
    let fixture = Fixture::new();
    populate(&fixture, BATCH + 3);
    advance(&fixture, 1).await;
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let (entered, paused) = tokio::sync::oneshot::channel();
    let (resume, wait) = tokio::sync::oneshot::channel();
    *worktree.prepared.lock().unwrap() = Some((entered, wait));
    let service = fixture.service.clone();
    let root = fixture.root.clone();
    let id = fixture.id;
    let stop = CancellationToken::new();
    let stopped = stop.clone();
    let indexing =
        tokio::spawn(async move { service.search(id, root, &Search::default(), &stopped).await });
    paused.await.unwrap();
    stop.cancel();
    resume.send(()).unwrap();
    assert_eq!(
        indexing.await.unwrap().unwrap_err().code,
        ErrorCode::Cancelled
    );
    let index = worktree.index().await.unwrap();
    assert_eq!(index.files().await.unwrap().len(), BATCH);
    let revision = index.outline("file_000.rs").await.unwrap().index_revision;
    assert_eq!(revision, 1);
    let resumed = fixture.search().await;
    assert!(!resumed.partial);
    assert_eq!(index.files().await.unwrap().len(), BATCH + 3);
    assert_eq!(resumed.index_revision, revision + 1);
}

#[tokio::test]
async fn rolls_back_interrupted_symbol_replacement() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn preserved() {}\n");
    let first = fixture.search().await;
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    let (_release, entered) = index.hold_fixture();
    let mut source = String::from("pub fn refreshed() {}\n");
    for number in 0..=index::BATCH_SYMBOLS {
        source.push_str(&format!("pub fn fresh_{number}() {{}}\n"));
    }
    fixture.write("main.rs", &source);
    fixture.service.changed(fixture.id, Some("main.rs"));
    let service = fixture.service.clone();
    let root = fixture.root.clone();
    let id = fixture.id;
    let stop = CancellationToken::new();
    let stopped = stop.clone();
    let indexing =
        tokio::spawn(async move { service.search(id, root, &Search::default(), &stopped).await });
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    // Caller cancellation drops a real open replacement transaction after its
    // metadata and multiple parameterized FTS statements have changed.
    stop.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), indexing)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    let outline = index.outline("main.rs").await.unwrap();
    assert_eq!(outline.index_revision, first.index_revision);
    assert_eq!(
        outline.file_revision,
        Some(first.symbols[0].file_revision.clone())
    );
    assert!(
        outline
            .symbols
            .iter()
            .any(|symbol| symbol.name == "preserved")
    );
    for (query, expected) in [
        ("preserved", first.symbols.clone()),
        ("refreshed", Vec::new()),
        ("fresh_512", Vec::new()),
    ] {
        let page = index
            .search(
                &Search {
                    query: query.into(),
                    ..Default::default()
                },
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(page.symbols, expected);
    }
    let resumed = fixture.search().await;
    assert!(
        resumed
            .symbols
            .iter()
            .any(|symbol| symbol.name == "refreshed")
    );
    assert_eq!(resumed.index_revision, first.index_revision + 1);
    let last = index
        .search(
            &Search {
                query: "fresh_512".into(),
                name: Some("fresh_512".into()),
                ..Default::default()
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(last.symbols.len(), 1);
    assert_eq!(last.symbols[0].name, "fresh_512");
}

#[tokio::test]
async fn finishes_atomic_writes_after_budget() {
    let fixture = Fixture::new();
    let source: String = (0..=index::BATCH_SYMBOLS)
        .map(|number| format!("pub fn item_{number}() {{}}\n"))
        .collect();
    fixture.write("first.rs", &source);
    fixture.write("second.rs", "pub fn second() {}\n");
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    let resources = fixture.service.resources().await.unwrap();
    let (release, entered) = index.hold_fixture();
    let deadline = Instant::now() + Duration::from_secs(1);
    let service = fixture.service.clone();
    let active = worktree.clone();
    let parsed = resources.clone();
    let indexing = tokio::spawn(async move {
        let mut progress = active.sync.lock().await;
        service
            .synchronize(
                &active,
                active.index().await.unwrap(),
                parsed,
                &CancellationToken::new(),
                &mut progress,
                sync::Limits {
                    deadline,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep_until((deadline + Duration::from_millis(10)).into()).await;
    assert!(!indexing.is_finished());
    release.send(()).unwrap();
    let report = indexing.await.unwrap();
    assert!(report.partial && report.truncated);
    let files = index.files().await.unwrap();
    assert_eq!(files.len(), 1);
    assert!(files.contains_key("first.rs"));
    assert_eq!(worktree.sync.lock().await.remaining(), Some(1));
    assert_eq!(worktree.sync.lock().await.next(), Some("second.rs"));
    let attempts = resources.parser.parsed.load(Ordering::Relaxed);
    advance(&fixture, usize::MAX).await;
    assert_eq!(index.files().await.unwrap().len(), 2);
    assert_eq!(
        resources.parser.parsed.load(Ordering::Relaxed),
        attempts + 1
    );
}

#[tokio::test]
async fn indexes_large_bundled_script() {
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/mobile/assets/visuals/mathjax.js"),
    )
    .unwrap();
    assert!(source.len() > 128 * 1024 && source.len() <= MAX_FILE_BYTES);
    let fixture = Fixture::new();
    fixture.write("mathjax.js", &source);
    let page = tokio::time::timeout(Duration::from_secs(120), fixture.search())
        .await
        .expect("the bundled script must finish within the admitted query deadline");
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    let files = index.files().await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files["mathjax.js"].state.status, Status::Parsed);
    assert_eq!(
        files["mathjax.js"].hash.as_deref(),
        Some(blake3::hash(source.as_bytes()).to_hex().as_str())
    );
    assert!(!page.symbols.is_empty());
    assert_eq!(index.outline("mathjax.js").await.unwrap().index_revision, 1);
    let repeated = fixture.search().await;
    assert_eq!(repeated.index_revision, page.index_revision);
}
