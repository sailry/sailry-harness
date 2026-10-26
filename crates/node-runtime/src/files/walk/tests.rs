use super::*;
use sailry_link::CancellationToken;
use std::time::{Duration, Instant};

fn directory() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}

fn control() -> Control {
    Control {
        cancelled: CancellationToken::new(),
        deadline: Instant::now() + Duration::from_secs(20),
    }
}

fn find(root: &Path, patterns: &[String], control: &Control) -> Result<FileNames, Fault> {
    super::find(root, patterns, "", MAX_FIND_MATCHES, 0, control)
}

fn timestamp(path: &Path, seconds: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
        )
        .unwrap();
}

#[test]
fn matches_names_without_reading_contents() {
    let root = directory();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    std::fs::write(root.path().join("src/b.rs"), [0xff, 0x00]).unwrap();
    std::fs::write(root.path().join("src/a.rs"), "").unwrap();
    std::fs::write(root.path().join("src/c.txt"), "").unwrap();
    std::fs::write(root.path().join(".git/hidden.rs"), "").unwrap();
    timestamp(&root.path().join("src/a.rs"), 1000);
    timestamp(&root.path().join("src/b.rs"), 1000);
    let result = find(
        &root.path().canonicalize().unwrap(),
        &["**/*.rs".into()],
        &control(),
    )
    .unwrap();
    assert_eq!(result.paths, ["src/a.rs", "src/b.rs"]);
    assert!(!result.truncated);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.visited, 5);
}

#[test]
fn directs_metadata_reads_to_git() {
    let root = directory();
    let error = collect(root.path(), ".git", &control()).err().unwrap();
    assert_eq!(error.code,ErrorCode::PermissionDenied);
    assert!(error.message.contains("Git branches tool"));
    assert!(error.message.contains("remote names and URLs"));
}

#[test]
fn bounds_results() {
    let root = directory();
    for index in 0..=MAX_FIND_MATCHES {
        let file = root.path().join(format!("{index:04}.rs"));
        std::fs::write(&file, "").unwrap();
        timestamp(&file, 1000);
    }
    let result = super::find(
        &root.path().canonicalize().unwrap(),
        &["**/*".into()],
        "",
        1_000_000,
        0,
        &control(),
    )
    .unwrap();
    assert_eq!(result.paths.len(), MAX_FIND_MATCHES);
    assert!(result.truncated);
    assert!(result.paths.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn rejects_patterns_and_observes_cancellation() {
    let root = directory();
    let path = root.path().canonicalize().unwrap();
    for patterns in [
        Vec::new(),
        vec![String::new()],
        vec!["**/*".into(), String::new()],
        vec!["[".into()],
    ] {
        assert_eq!(
            find(&path, &patterns, &control()).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    let control = control();
    control.cancelled.cancel();
    assert_eq!(
        find(&path, &["**/*".into()], &control).unwrap_err().code,
        ErrorCode::Cancelled
    );
}

#[test]
fn accepts_large_globs_without_shortening_patterns() {
    let root = directory();
    let path = root.path().canonicalize().unwrap();
    for patterns in [vec!["🙂".repeat(4097)], vec!["*.rs".into(); 65]] {
        let result = find(&path, &patterns, &control()).unwrap();
        assert!(result.paths.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn does_not_follow_symlinks() {
    let root = directory();
    let outside = directory();
    std::fs::write(outside.path().join("private.rs"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("private.rs"),
        root.path().join("private.rs"),
    )
    .unwrap();
    let result = find(
        &root.path().canonicalize().unwrap(),
        &["**/*".into()],
        &control(),
    )
    .unwrap();
    assert!(result.paths.is_empty());
    assert_eq!(result.skipped, 2);
}

#[cfg(unix)]
#[test]
fn rejects_linked_roots_and_scopes() {
    let root = directory();
    let outside = directory();
    std::fs::write(outside.path().join("private.rs"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    assert!(find(&root.path().join("linked"), &["**/*.rs".into()], &control()).is_err());
    assert!(
        super::find(
            root.path(),
            &["**/*.rs".into()],
            "linked",
            500,
            0,
            &control()
        )
        .is_err()
    );
    let result = find(root.path(), &["**/*.rs".into()], &control()).unwrap();
    assert!(result.paths.is_empty());
    assert_eq!(result.skipped, 1);
    assert!(!result.partial);
}

#[test]
fn inherits_ignores_with_negation_and_subpaths() {
    let root = directory();
    std::fs::create_dir_all(root.path().join("src/nested")).unwrap();
    std::fs::create_dir(root.path().join("excluded")).unwrap();
    std::fs::write(
        root.path().join(".gitignore"),
        "*.tmp\n/excluded/\n/root.rs\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/.gitignore"),
        "!keep.tmp\n/nested/skip.rs\n",
    )
    .unwrap();
    for file in [
        "root.rs",
        "src/root.rs",
        "src/a.tmp",
        "src/keep.tmp",
        "src/nested/skip.rs",
        "src/nested/use.rs",
        "excluded/private.rs",
    ] {
        std::fs::write(root.path().join(file), "").unwrap();
    }
    let result = super::find(
        root.path(),
        &["**/*".into()],
        "src/nested",
        500,
        0,
        &control(),
    )
    .unwrap();
    assert_eq!(result.paths, ["src/nested/use.rs"]);
    let result = super::find(root.path(), &["**/*.tmp".into()], "src", 500, 0, &control()).unwrap();
    assert_eq!(result.paths, ["src/keep.tmp"]);
    let result = super::find(root.path(), &["**/*.rs".into()], "", 500, 0, &control()).unwrap();
    let mut paths = result.paths;
    paths.sort();
    assert_eq!(paths, ["src/nested/use.rs", "src/root.rs"]);
    let result = super::find(
        root.path(),
        &["**/*".into()],
        "src/a.tmp",
        500,
        0,
        &control(),
    )
    .unwrap();
    assert!(result.paths.is_empty());
}

#[test]
fn distinguishes_missing_and_ignored_scopes() {
    let root = directory();
    let repository = git2::Repository::init(root.path()).unwrap();
    let patterns = ["**/*.py".into()];
    assert_eq!(
        super::find(root.path(), &patterns, "scripts", 500, 0, &control())
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let mut module = repository
        .submodule(
            "https://example.invalid/scripts.git",
            Path::new("scripts"),
            true,
        )
        .unwrap();
    let child = module.open().unwrap();
    std::fs::write(root.path().join("scripts/task.py"), "print('fixture')\n").unwrap();
    let mut index = child.index().unwrap();
    index.add_path(Path::new("task.py")).unwrap();
    index.write().unwrap();
    let tree = child.find_tree(index.write_tree().unwrap()).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    child
        .commit(Some("HEAD"), &author, &author, "Fixture", &tree, &[])
        .unwrap();
    module.add_finalize().unwrap();
    let names = super::find(root.path(), &patterns, "scripts", 500, 0, &control()).unwrap();
    assert_eq!(names.paths, ["scripts/task.py"]);
    assert!(!names.partial);
    assert!(!names.truncated);
    assert_eq!(names.skipped, 0);
    assert!(names.diagnostics.is_empty());
    let names = super::find(root.path(), &["**/*".into()], "scripts", 500, 0, &control()).unwrap();
    assert_eq!(names.paths, ["scripts/task.py"]);
    for scope in [".git", "scripts/.git"] {
        assert_eq!(
            super::find(root.path(), &patterns, scope, 500, 0, &control())
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
    std::fs::create_dir(root.path().join("ignored")).unwrap();
    std::fs::write(root.path().join("ignored/private.py"), "private fixture\n").unwrap();
    std::fs::write(root.path().join(".gitignore"), "/ignored/\n").unwrap();
    let names = super::find(root.path(), &patterns, "ignored", 500, 0, &control()).unwrap();
    assert!(names.paths.is_empty());
    assert!(!names.partial);
    assert!(!names.truncated);
    assert_eq!(names.skipped, 0);
    assert!(names.diagnostics.is_empty());
}

#[test]
fn orders_by_modification_then_path_and_pages() {
    let root = directory();
    for (name, modified) in [("old.rs", 1000), ("b.rs", 2000), ("a.rs", 2000)] {
        std::fs::write(root.path().join(name), "").unwrap();
        timestamp(&root.path().join(name), modified);
    }
    let first = super::find(root.path(), &["**/*.rs".into()], "", 1, 0, &control()).unwrap();
    assert_eq!(first.paths, ["a.rs"]);
    assert_eq!(first.next_offset, Some(1));
    let second = super::find(root.path(), &["**/*.rs".into()], "", 2, 1, &control()).unwrap();
    assert_eq!(second.paths, ["b.rs", "old.rs"]);
    assert!(second.next_offset.is_none());
}

#[test]
fn visits_more_than_ten_thousand_entries() {
    let root = directory();
    for index in 0..10_010 {
        std::fs::write(root.path().join(format!("{index:05}.rs")), "").unwrap();
    }
    let result = super::find(root.path(), &["10009.rs".into()], "", 500, 0, &control()).unwrap();
    assert_eq!(result.paths, ["10009.rs"]);
    assert!(!result.partial, "{result:?}");
    assert_eq!(result.visited, 10_010);
}

#[test]
fn interrupted_walk_has_no_safe_continuation() {
    let root = directory();
    for name in ["a.rs", "b.rs", "c.rs"] {
        std::fs::write(root.path().join(name), "").unwrap();
    }
    let control = control();
    let mut paths = Vec::new();
    let report = traverse(
        root.path(),
        "",
        true,
        &control,
        &mut |_, _, relative, _| {
            paths.push(relative.to_owned());
            control.cancelled.cancel();
            Ok(true)
        },
        &mut |_, _| Ok(()),
    )
    .unwrap();
    assert_eq!(paths, ["a.rs"]);
    assert!(report.partial);
    assert!(report.truncated);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|item| item.message.contains("narrow path"))
    );
}

#[cfg(unix)]
#[test]
fn never_reads_symlinked_ignore_rules() {
    let root = directory();
    let outside = directory();
    std::fs::write(outside.path().join("rules"), "*.rs\n").unwrap();
    std::os::unix::fs::symlink(outside.path().join("rules"), root.path().join(".gitignore"))
        .unwrap();
    std::fs::write(root.path().join("visible.rs"), "").unwrap();
    let result = find(root.path(), &["**/*.rs".into()], &control()).unwrap();
    assert_eq!(result.paths, ["visible.rs"]);
    assert!(result.partial);
    assert!(result.next_offset.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.message.contains("not a regular file"))
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|item| item.message.contains("*.rs"))
    );
}
