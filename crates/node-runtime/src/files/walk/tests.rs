use super::*;
use sailry_link::CancellationToken;
use std::time::{Duration, Instant};

fn control() -> Control {
    Control {
        cancelled: CancellationToken::new(),
        deadline: Instant::now() + Duration::from_secs(5),
    }
}

#[test]
fn matches_names_without_reading_contents() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    std::fs::write(root.path().join("src/b.rs"), [0xff, 0x00]).unwrap();
    std::fs::write(root.path().join("src/a.rs"), "").unwrap();
    std::fs::write(root.path().join("src/c.txt"), "").unwrap();
    std::fs::write(root.path().join(".git/hidden.rs"), "").unwrap();
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
fn bounds_results() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..=MAX_FIND_MATCHES {
        std::fs::write(root.path().join(format!("{index:04}.rs")), "").unwrap();
    }
    let result = find(
        &root.path().canonicalize().unwrap(),
        &["**/*".into()],
        &control(),
    )
    .unwrap();
    assert_eq!(result.paths.len(), MAX_FIND_MATCHES);
    assert!(result.truncated);
    assert!(result.paths.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn rejects_patterns_and_observes_cancellation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    for patterns in [
        Vec::new(),
        vec![String::new()],
        vec!["**/*".into(), String::new()],
        vec!["[".into()],
        vec!["**/*".into(); FileSearch::MAX_GLOBS + 1],
        vec!["x".repeat(4097)],
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
fn measures_globs_like_the_script_handler() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let result = find(&path, &["🙂".repeat(2048)], &control()).unwrap();
    assert!(result.paths.is_empty());
    assert_eq!(
        find(&path, &["🙂".repeat(2049)], &control())
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest,
    );
}

#[cfg(unix)]
#[test]
fn does_not_follow_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
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
