use super::*;
use std::time::{Duration, Instant};

fn control() -> Control {
    Control {
        cancelled: sailry_link::CancellationToken::new(),
        deadline: Instant::now() + Duration::from_secs(5),
    }
}

#[test]
fn preserves_classification_without_refreshing_index() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("tracked.txt"), "before\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("tracked.txt")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(Some("HEAD"), &author, &author, "Initial", &tree, &[])
        .unwrap();
    std::fs::write(root.join("tracked.txt"), "after\n").unwrap();
    std::fs::write(root.join("new file 中文.txt"), "untracked\n").unwrap();
    std::fs::write(root.join("staged.txt"), "staged\n").unwrap();
    index.add_path(Path::new("staged.txt")).unwrap();
    index.write().unwrap();
    std::fs::write(root.join("intent.txt"), "intent\n").unwrap();
    assert!(
        std::process::Command::new("git")
            .current_dir(&root)
            .args(["add", "-N", "intent.txt"])
            .status()
            .unwrap()
            .success()
    );
    let mut config = repository.config().unwrap();
    config
        .set_str("core.fsmonitor", "sailry-monitor-must-not-run")
        .unwrap();
    config
        .set_str("filter.fixture.clean", "sailry-filter-must-not-run")
        .unwrap();
    config
        .set_str("filter.fixture.process", "sailry-filter-must-not-run")
        .unwrap();
    config.set_bool("filter.fixture.required", true).unwrap();
    std::fs::write(root.join(".gitattributes"), "*.txt filter=fixture\n").unwrap();
    let before = std::fs::read(repository.path().join("index")).unwrap();
    let expected = super::super::inspect(&root, &control()).unwrap();
    let actual = inspect(&root, &crate::process::Environment::ambient(), &control()).unwrap();
    assert_eq!(actual.kind, expected.kind);
    assert_eq!(actual.head, expected.head);
    assert_eq!(actual.branch, expected.branch);
    assert_eq!(actual.index_revision, expected.index_revision);
    let classifications = |status: &GitStatus| {
        status
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.path.clone(),
                    entry.staged,
                    entry.unstaged,
                    entry.untracked,
                    entry.conflicted,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(classifications(&actual), classifications(&expected));
    assert!(actual.entries.iter().all(|entry| entry.diff.truncated));
    assert!(!actual.truncated);
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        before
    );
}

#[test]
fn preserves_complete_records_at_output_limit() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let text = "# branch.oid (initial)\0# branch.head main\0? first.txt\0? incomplete";
    let result = parse(text, true, &root, &control()).unwrap();
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.entries[0].path, "first.txt");
    assert!(result.truncated);
    assert_eq!(result.omitted_paths, 1);
    assert!(parse(text, false, &root, &control()).is_err());
}

#[test]
fn classifies_unmerged_records_and_confined_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let text = "# branch.oid 1111111111111111111111111111111111111111\0# branch.head (detached)\0u UU N... 100644 100644 100644 100644 a b c conflict.txt\0? ../escape\0? .sailry/worktrees/other/file\0";
    let result = parse(text, false, &root, &control()).unwrap();
    assert_eq!(result.kind, RepositoryKind::Ready);
    assert!(result.branch.is_none());
    assert_eq!(result.entries.len(), 1);
    assert!(result.entries[0].conflicted);
    assert!(result.entries[0].staged_diff.truncated);
    assert!(result.entries[0].unstaged_diff.truncated);
    assert_eq!(result.omitted_paths, 2);
}

#[test]
fn observes_cancellation_before_native_execution() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    Repository::init(&root).unwrap();
    let control = control();
    control.cancelled.cancel();
    assert_eq!(
        inspect(&root, &crate::process::Environment::ambient(), &control)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}
