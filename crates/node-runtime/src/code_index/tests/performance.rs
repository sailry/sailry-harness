//! Explicit measurements stay separate from functional checks and CI.
use super::*;
use std::sync::atomic::Ordering;

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
    let first = fixture.search().await;
    let initial = start.elapsed();
    let resources = fixture.service.resources().await.unwrap();
    let first_parsed = resources.parser.parsed.load(Ordering::Relaxed);
    assert_eq!(first_parsed, files);
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
        "Code index synthetic corpus: {files} Rust files, {bytes} bytes; first={initial:?} (parsed={first_parsed}, file_batch_commits={}), unchanged={unchanged:?} (parsed={}, file_batch_commits={}), single_update={update:?} (parsed={}, file_batch_commits={}); target={}-{}; verified parser preparation and fixture creation excluded",
        first.index_revision,
        reused_parsed - first_parsed,
        reused.index_revision - first.index_revision,
        updated_parsed - reused_parsed,
        updated.index_revision - reused.index_revision,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}
