use super::*;

mod budgets;
mod performance;
mod updates;

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    id: WorktreeId,
    service: Arc<Service>,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("profile");
        crate::profile::private_directory(&profile).unwrap();
        let root = directory.path().join("repository");
        std::fs::create_dir(&root).unwrap();
        let service = Service::new(
            Some(profile),
            NodeId([81; 32]),
            Arc::default(),
            CancellationToken::new(),
        );
        Self {
            _directory: directory,
            root: root.canonicalize().unwrap(),
            id: WorktreeId::new(),
            service,
        }
    }

    fn write(&self, path: &str, text: &str) {
        std::fs::write(self.root.join(path), text).unwrap();
    }

    async fn search(&self) -> SearchPage {
        self.service
            .search(
                self.id,
                self.root.clone(),
                &Search::default(),
                &CancellationToken::new(),
            )
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn outline_parses_only_the_requested_file() {
    let fixture = Fixture::new();
    fixture.write("first.rs", "pub fn first() {}\n");
    fixture.write("second.rs", "pub fn second() {}\n");
    let outline = fixture
        .service
        .outline(
            fixture.id,
            fixture.root.clone(),
            "first.rs",
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(outline.symbols.iter().any(|symbol| symbol.name == "first"));
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let files = worktree.index().await.unwrap().files().await.unwrap();
    assert_eq!(files.into_keys().collect::<Vec<_>>(), ["first.rs"]);
    assert!(
        fixture
            .search()
            .await
            .symbols
            .iter()
            .any(|symbol| symbol.name == "second")
    );
}

#[tokio::test]
async fn reuses_hashes_and_extraction_fingerprints() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn original() {}\n");
    let first = fixture.search().await;
    assert_eq!(fixture.search().await.index_revision, first.index_revision);
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    let index = worktree.index().await.unwrap();
    let resources = fixture.service.resources().await.unwrap();
    let file = resources
        .parser
        .parse(
            "main.rs".into(),
            "pub fn original() {}\n",
            &CancellationToken::new(),
        )
        .unwrap();
    index
        .replace(
            &[file],
            "different-extraction-rules",
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let before = index.outline("main.rs").await.unwrap().index_revision;
    assert!(fixture.search().await.index_revision > before);
    assert_eq!(
        index.files().await.unwrap()["main.rs"].fingerprint,
        resources.fingerprint
    );
}

#[tokio::test]
async fn replaces_native_fts_and_metadata_together() {
    let fixture = Fixture::new();
    fixture.write("main.rs", "pub fn retired() {}\n");
    fixture.search().await;
    fixture.write("main.rs", "pub fn current() {}\n");
    fixture.service.changed(fixture.id, Some("main.rs"));
    let current = fixture.search().await;
    assert!(
        current
            .symbols
            .iter()
            .all(|symbol| symbol.name != "retired")
    );
    let page = fixture
        .service
        .search(
            fixture.id,
            fixture.root.clone(),
            &Search {
                query: "retired".into(),
                ..Default::default()
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(page.symbols.is_empty());
    let outline = fixture
        .service
        .outline(
            fixture.id,
            fixture.root.clone(),
            "main.rs",
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let revision = outline.file_revision.unwrap();
    assert!(!outline.symbols.is_empty());
    assert!(
        outline
            .symbols
            .iter()
            .all(|symbol| symbol.file_revision == revision)
    );
    assert_eq!(outline.index_revision, current.index_revision);
}

#[test]
fn scopes_cancellation_and_catches_early_delivery() {
    let queries = Arc::new(Queries::default());
    let caller = NodeId([82; 32]);
    let request = sailry_protocol::RequestId::new();
    queries.cancel(caller, request);
    assert_eq!(
        queries
            .begin(caller, request, &CancellationToken::new())
            .err()
            .unwrap()
            .code,
        ErrorCode::Cancelled
    );
    let active = queries
        .begin(caller, request, &CancellationToken::new())
        .unwrap();
    queries.cancel(NodeId([83; 32]), request);
    assert!(!active.stop.is_cancelled());
    queries.cancel(caller, request);
    assert!(active.stop.is_cancelled());
    let stopped = active.stop.clone();
    drop(active);
    assert!(stopped.is_cancelled());
    assert!(
        queries
            .begin(caller, request, &CancellationToken::new())
            .is_ok()
    );
}

#[test]
fn retains_a_new_dirty_generation() {
    let mut dirty = Dirty::default();
    dirty.change(Some("main.rs"));
    let captured = dirty.generation;
    dirty.change(Some("second.rs"));
    assert!(!dirty.finish(captured));
    assert!(dirty.paths.contains_key("main.rs") && dirty.paths.contains_key("second.rs"));
    assert_ne!(dirty.clean, dirty.generation);
    assert!(dirty.finish(dirty.generation));
    assert!(dirty.paths.is_empty());
}

#[tokio::test]
async fn removes_loaded_and_unloaded_caches() {
    for loaded in [true, false] {
        let fixture = Fixture::new();
        fixture.write("main.rs", "pub fn indexed() {}\n");
        fixture.search().await;
        let cache = fixture
            .service
            .profile
            .as_ref()
            .unwrap()
            .join("cache/code-index")
            .join(fixture.id.to_string());
        assert!(cache.join("index.turso").is_file());
        if !loaded {
            fixture.service.configure(false);
        }
        fixture.service.remove(fixture.id).await.unwrap();
        assert!(!cache.exists());
        assert!(fixture.root.join("main.rs").is_file());
        fixture.service.remove(fixture.id).await.unwrap();
    }
}
