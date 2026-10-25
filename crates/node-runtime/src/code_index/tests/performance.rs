//! Explicit measurements stay separate from functional checks and CI.
use super::*;
use std::sync::atomic::Ordering;

async fn complete(fixture: &Fixture) -> SearchPage {
    loop {
        let page = fixture.search().await;
        let worktree = fixture
            .service
            .worktree(fixture.id, fixture.root.clone())
            .unwrap();
        if worktree.sync.lock().await.remaining().is_none() {
            return page;
        }
    }
}

#[tokio::test]
#[ignore = "opt-in code index performance measurement"]
async fn measures_first_reuse_and_update() {
    let fixture = Fixture::new();
    let files = 1000;
    let mut bytes = 0usize;
    for file in 0..files {
        fixture.write(
            &format!("file_{file}.rs"),
            &format!(
                "pub struct Item{file};\npub fn item_{file}() -> Item{file} {{ Item{file} }}\n"
            ),
        );
    }
    for entry in std::fs::read_dir(&fixture.root).unwrap() {
        bytes += entry.unwrap().metadata().unwrap().len() as usize;
    }
    let start = Instant::now();
    let first = complete(&fixture).await;
    let initial = start.elapsed();
    let resources = fixture.service.resources().await.unwrap();
    let first_parsed = resources.parser.parsed.load(Ordering::Relaxed);
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    assert_eq!(
        worktree.index().await.unwrap().files().await.unwrap().len(),
        files
    );
    assert!(first_parsed >= files);
    // A real root invalidation requires complete candidate hashes even when all match.
    fixture.service.changed(fixture.id, None);
    let start = Instant::now();
    let reused = fixture.search().await;
    let unchanged = start.elapsed();
    let reused_parsed = resources.parser.parsed.load(Ordering::Relaxed);
    assert_eq!(reused_parsed - first_parsed, 0);
    assert_eq!(reused.index_revision, first.index_revision);
    fixture.write("file_0.rs", "pub fn updated() {}\n");
    fixture.service.changed(fixture.id, Some("file_0.rs"));
    let start = Instant::now();
    let updated = fixture.search().await;
    let update = start.elapsed();
    let updated_parsed = resources.parser.parsed.load(Ordering::Relaxed);
    assert_eq!(updated_parsed - reused_parsed, 1);
    assert!(updated.index_revision > reused.index_revision);
    assert_eq!(updated.index_revision - reused.index_revision, 1);
    eprintln!(
        "Code index synthetic corpus: {files} Rust files, {bytes} bytes; first={initial:?} (parse_attempts={first_parsed}, file_batch_commits={}), unchanged={unchanged:?} (parse_attempts={}, file_batch_commits={}), single_update={update:?} (parse_attempts={}, file_batch_commits={}); target={}-{}; verified parser preparation and fixture creation excluded",
        first.index_revision,
        reused_parsed - first_parsed,
        reused.index_revision - first.index_revision,
        updated_parsed - reused_parsed,
        updated.index_revision - reused.index_revision,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

#[tokio::test]
#[ignore = "opt-in read-only repository code index measurement"]
async fn measures_repository_segments() {
    let source = std::env::var_os("SAILRY_CODE_INDEX_SOURCE")
        .expect("SAILRY_CODE_INDEX_SOURCE must identify the read-only repository");
    let mut fixture = Fixture::new();
    fixture.root = std::fs::canonicalize(source).unwrap();
    assert!(fixture.root.is_dir());
    let worktree = fixture
        .service
        .worktree(fixture.id, fixture.root.clone())
        .unwrap();
    // The temporary cache starts dirty; repository contents are never changed.
    fixture.service.changed(fixture.id, None);
    let mut segments = Vec::new();
    let started = Instant::now();
    loop {
        let start = Instant::now();
        let page = fixture.search().await;
        let elapsed = start.elapsed();
        let remaining = worktree.sync.lock().await.remaining();
        let indexed = worktree.index().await.unwrap().files().await.unwrap().len();
        let reconciled = {
            let dirty = worktree.dirty.lock().unwrap();
            dirty.clean == dirty.generation
        };
        segments.push((elapsed, indexed, remaining, reconciled, page));
        if remaining.is_none() && reconciled {
            break;
        }
    }
    let total = started.elapsed();
    let entries = worktree.index().await.unwrap().files().await.unwrap();
    let mut states = BTreeMap::<String, usize>::new();
    for file in entries.values() {
        *states
            .entry(format!("{:?}", file.state.status))
            .or_default() += 1;
    }
    // Emit after measurement so the log itself cannot invalidate the repository
    // watcher between segments when the destination is an ignored local folder.
    for (index, (elapsed, indexed, remaining, reconciled, page)) in segments.iter().enumerate() {
        eprintln!(
            "Repository segment {}: elapsed={elapsed:?}, indexed_files={indexed}, remaining={remaining:?}, reconciled={reconciled}, revision={}, partial={}, truncated={}, visited={}, skipped={}",
            index + 1,
            page.index_revision,
            page.partial,
            page.truncated,
            page.visited,
            page.skipped
        );
    }
    eprintln!(
        "Repository index: root={}, indexed_files={}, states={states:?}, total={total:?}, segments={}, parse_attempts={}, target={}-{}; cache/profile are temporary, source is read-only, metadata probes are outside segment latency, capability partial is distinct from cursor completion",
        fixture.root.display(),
        entries.len(),
        segments.len(),
        fixture
            .service
            .resources()
            .await
            .unwrap()
            .parser
            .parsed
            .load(Ordering::Relaxed),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}
