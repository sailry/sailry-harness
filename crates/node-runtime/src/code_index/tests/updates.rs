use super::*;

#[tokio::test]
async fn reuses_oversized_status_until_size_changes() {
    let fixture = Fixture::new();
    fixture.write("main.rs", &"x".repeat(MAX_FILE_BYTES + 1));
    let first = fixture.search().await;
    assert_eq!(first.issues[0].status, Status::TooLarge);
    fixture.service.changed(fixture.id, None);
    let checked = fixture.search().await;
    assert_eq!(checked.index_revision, first.index_revision);
    fixture.write("main.rs", "pub fn now_readable() {}\n");
    fixture.service.changed(fixture.id, Some("main.rs"));
    let parsed = fixture.search().await;
    assert_eq!(parsed.index_revision, first.index_revision + 1);
    assert!(
        parsed
            .symbols
            .iter()
            .any(|symbol| symbol.name == "now_readable")
    );
}

#[tokio::test]
async fn incomplete_walk_preserves_unvisited_records() {
    let fixture = Fixture::new();
    fixture.write("first.rs", "pub fn first() {}\n");
    fixture.write("last.rs", "pub fn last() {}\n");
    fixture.search().await;
    std::fs::remove_file(fixture.root.join("last.rs")).unwrap();
    fixture.write("other.rs", "pub fn other() {}\n");
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    let resources = fixture.service.resources().await.unwrap();
    let report = fixture
        .service
        .synchronize(
            &worktree,
            index,
            resources,
            &CancellationToken::new(),
            &mut *worktree.sync.lock().await,
            sync::Limits {
                files: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(report.partial && report.truncated);
    assert!(index.files().await.unwrap().contains_key("last.rs"));
    fixture.search().await;
    assert!(!index.files().await.unwrap().contains_key("last.rs"));
}

#[tokio::test]
async fn failed_walk_preserves_cached_records() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn preserved() {}\n");
    fixture.search().await;
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    std::fs::rename(&fixture.root, fixture.root.with_file_name("moved")).unwrap();
    let result = fixture
        .service
        .search(
            fixture.id,
            fixture.root.clone(),
            &Search::default(),
            &CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());
    assert!(
        worktree
            .index()
            .await
            .unwrap()
            .files()
            .await
            .unwrap()
            .contains_key("main.rs")
    );
}

#[tokio::test]
async fn changes_during_parse_remain_dirty() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn before() {}\n");
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
    fixture.write("main.rs", "pub fn after_() {}\n");
    fixture.service.changed(id, Some("main.rs"));
    resume.send(()).unwrap();
    let old = indexing.await.unwrap();
    assert!(old.partial);
    {
        let dirty = worktree.dirty.lock().unwrap();
        assert!(dirty.paths.contains_key("main.rs"));
    }
    let fresh = fixture.search().await;
    assert!(fresh.symbols.iter().any(|symbol| symbol.name == "after_"));
    assert!(fresh.symbols.iter().all(|symbol| symbol.name != "before"));
}

#[tokio::test]
async fn cancellation_prevents_commit_and_pruning() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn preserved() {}\n");
    let first = fixture.search().await;
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    fixture.write("main.rs", "pub fn discarded() {}\n");
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
    let cached = index.outline("main.rs").await.unwrap();
    assert_eq!(cached.index_revision, first.index_revision);
    assert!(
        cached
            .symbols
            .iter()
            .any(|symbol| symbol.name == "preserved")
    );
}

#[tokio::test]
async fn disabling_cancels_pending_work() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn preserved() {}\n");
    let first = fixture.search().await;
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    fixture.write("main.rs", "pub fn refreshed() {}\n");
    let (entered, paused) = tokio::sync::oneshot::channel();
    let (_resume, wait) = tokio::sync::oneshot::channel();
    *worktree.prepared.lock().unwrap() = Some((entered, wait));
    let service = fixture.service.clone();
    let root = fixture.root.clone();
    let id = fixture.id;
    let indexing = tokio::spawn(async move {
        service
            .search(id, root, &Search::default(), &CancellationToken::new())
            .await
    });
    paused.await.unwrap();
    fixture.service.configure(false);
    let result = tokio::time::timeout(Duration::from_secs(5), indexing)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert!(fixture.service.worktrees.lock().unwrap().is_empty());
    assert_eq!(
        worktree
            .index()
            .await
            .unwrap()
            .outline("main.rs")
            .await
            .unwrap()
            .index_revision,
        first.index_revision
    );
    assert_eq!(
        fixture
            .service
            .worktree(fixture.id, fixture.root.clone())
            .err()
            .unwrap()
            .code,
        ErrorCode::NotConfigured
    );
    fixture.service.configure(true);
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
async fn validates_full_hash_after_same_size_change() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn first() {}\n");
    let metadata = std::fs::metadata(fixture.root.join("main.rs")).unwrap();
    let stamp = metadata.modified().unwrap();
    let first = fixture.search().await;
    fixture.write("main.rs", "pub fn other() {}\n");
    std::fs::File::options()
        .write(true)
        .open(fixture.root.join("main.rs"))
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(stamp))
        .unwrap();
    assert_eq!(
        std::fs::metadata(fixture.root.join("main.rs"))
            .unwrap()
            .len(),
        metadata.len()
    );
    let other = fixture.search().await;
    assert!(other.symbols.iter().any(|symbol| symbol.name == "other"));
    assert_ne!(
        first.symbols[0].file_revision,
        other.symbols[0].file_revision
    );
    assert!(other.index_revision > first.index_revision);
}
