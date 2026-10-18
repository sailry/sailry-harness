mod context;
use super::*;
use git2::{Repository, Signature};
use std::{
    fs,
    time::{Duration, Instant},
};

struct Fixture {
    directory: tempfile::TempDir,
    repository: Repository,
    control: Control,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path()).unwrap();
        Self {
            directory,
            repository,
            control: Control {
                cancelled: sailry_link::CancellationToken::new(),
                deadline: Instant::now() + Duration::from_secs(30),
            },
        }
    }

    fn root(&self) -> std::path::PathBuf {
        self.directory.path().canonicalize().unwrap()
    }

    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let destination = self.root().join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, bytes).unwrap();
    }

    fn commit(&self) -> String {
        let mut index = self.repository.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree = self
            .repository
            .find_tree(index.write_tree().unwrap())
            .unwrap();
        let signature = Signature::now("Fixture", "fixture@example.test").unwrap();
        let parent = self
            .repository
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok());
        self.repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Fixture",
                &tree,
                &parent.iter().collect::<Vec<_>>(),
            )
            .unwrap()
            .to_string()
    }

    #[cfg(unix)]
    fn commit_with_cli(&self) {
        for arguments in [
            &["add", "--", "."][..],
            &["commit", "--no-gpg-sign", "-m", "Fixture"][..],
        ] {
            let output = std::process::Command::new("git")
                .current_dir(self.root())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.test",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(arguments)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Git fixture failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    fn working(&self, limit: usize) -> GitChanges {
        list(
            &self.root(),
            Some(GitChangeTarget::Worktree {
                scope: GitDiffScope::All,
            }),
            None,
            limit,
            &self.control,
        )
        .unwrap()
    }

    fn read(
        &self,
        inventory: &GitChanges,
        name: &str,
        revision: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<GitChange, Fault> {
        let metadata = &inventory
            .entries
            .iter()
            .find(|entry| entry.path == name)
            .unwrap()
            .metadata_revision;
        read(
            &self.root(),
            inventory.snapshot.clone(),
            name,
            metadata,
            Page {
                expected_revision: revision,
                offset,
                limit,
            },
            &self.control,
        )
    }
}

#[test]
fn resolves_long_revision_expressions() {
    let fixture = Fixture::new();
    fixture.write("file.txt", "original");
    let commit = fixture.commit();
    let revision = format!("HEAD{}", "~0".repeat(600));
    let result = list(
        &fixture.root(),
        Some(GitChangeTarget::Commit { revision }),
        None,
        1,
        &fixture.control,
    )
    .unwrap();
    let GitChangeState::Commit { revision, .. } = result.snapshot.target else {
        panic!("commit snapshot expected")
    };
    assert_eq!(revision, commit);
}

#[test]
fn enumerates_every_page_without_bodies() {
    let fixture = Fixture::new();
    for number in 0..1_203 {
        fixture.write(&format!("file-{number:04}.rs"), [0, 255]);
    }
    let mut result = fixture.working(1_000_000);
    let mut paths = Vec::new();
    loop {
        assert!(encoded(&Output::GitChanges(result.clone())).unwrap() <= MAX_DIFF_BYTES);
        assert!(result.entries.len() <= MAX_GIT_LINES);
        assert_eq!(result.omitted_paths, 0);
        paths.extend(result.entries.iter().map(|entry| entry.path.clone()));
        let Some(cursor) = result.next else {
            assert!(result.exhausted);
            break;
        };
        assert!(!result.exhausted);
        result = list(
            &fixture.root(),
            None,
            Some(cursor),
            1_000_000,
            &fixture.control,
        )
        .unwrap();
    }
    assert_eq!(paths.len(), 1_203);
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(paths.iter().all(|path| path.ends_with(".rs")));
}

#[test]
fn freezes_commit_and_first_parent() {
    let fixture = Fixture::new();
    fixture.write("file.rs", "first\n");
    let first = fixture.commit();
    let scope = list(
        &fixture.root(),
        Some(GitChangeTarget::Commit {
            revision: "HEAD".into(),
        }),
        None,
        10,
        &fixture.control,
    )
    .unwrap();
    assert_eq!(
        scope.snapshot.target,
        GitChangeState::Commit {
            revision: first.clone(),
            parent: None
        }
    );
    fixture.write("file.rs", "second\n");
    let second = fixture.commit();
    let repeated = list(
        &fixture.root(),
        None,
        Some(GitChangesCursor {
            snapshot: scope.snapshot.clone(),
            after_path: None,
        }),
        10,
        &fixture.control,
    )
    .unwrap();
    assert_eq!(repeated, scope);
    let content = fixture.read(&scope, "file.rs", None, 0, 500).unwrap();
    assert_eq!(content.lines[0].text, "first");
    assert_eq!(content.lines[0].old_line, None);
    assert_eq!(content.lines[0].new_line, Some(1));
    let range = list(
        &fixture.root(),
        Some(GitChangeTarget::Range {
            base: first.clone(),
            head: second.clone(),
        }),
        None,
        10,
        &fixture.control,
    )
    .unwrap();
    assert_eq!(
        range.snapshot.target,
        GitChangeState::Range {
            base: first,
            head: second
        }
    );
    assert_eq!(
        fixture
            .read(&range, "file.rs", None, 0, 500)
            .unwrap()
            .lines
            .len(),
        2
    );
}

#[test]
fn preserves_literal_paths_and_line_mapping() {
    let fixture = Fixture::new();
    fixture.write("-file[1].rs", "before\r\n  retained \r\n");
    fixture.write("file1.rs", "unrelated\n");
    fixture.commit();
    fixture.write("-file[1].rs", "after\r\n  retained \r\n");
    let scope = fixture.working(500);
    let content = fixture.read(&scope, "-file[1].rs", None, 0, 500).unwrap();
    assert_eq!(content.first_line.as_deref(), Some("after"));
    assert_eq!(
        content
            .lines
            .iter()
            .map(|line| (line.kind, line.old_line, line.new_line, line.text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (GitChangeLineKind::Deleted, Some(1), None, "before"),
            (GitChangeLineKind::Added, None, Some(1), "after"),
            (GitChangeLineKind::Context, Some(2), Some(2), "  retained "),
        ]
    );
    for name in ["../outside", "/etc/passwd", ".git/config", "file*.rs"] {
        assert!(
            read(
                &fixture.root(),
                scope.snapshot.clone(),
                name,
                &scope.entries[0].metadata_revision,
                Page {
                    expected_revision: None,
                    offset: 0,
                    limit: 100
                },
                &fixture.control
            )
            .is_err()
        );
    }
}

#[test]
fn requires_revision_on_mapped_continuations() {
    let fixture = Fixture::new();
    fixture.write(
        "file.rs",
        (0..603)
            .map(|line| format!("line {line}\n"))
            .collect::<String>(),
    );
    let scope = fixture.working(500);
    let first = fixture.read(&scope, "file.rs", None, 0, 1_000_000).unwrap();
    assert_eq!(first.lines.len(), MAX_GIT_LINES);
    assert_eq!(first.next, Some(MAX_GIT_LINES));
    assert_eq!(
        fixture
            .read(&scope, "file.rs", None, MAX_GIT_LINES, 1_000_000)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let mut lines = first.lines;
    let mut next = first.next;
    while let Some(offset) = next {
        let page = fixture
            .read(&scope, "file.rs", Some(&first.revision), offset, 1_000_000)
            .unwrap();
        assert!(encoded(&Output::GitChange(page.clone())).unwrap() <= MAX_DIFF_BYTES);
        next = page.next;
        lines.extend(page.lines);
    }
    assert_eq!(lines.len(), 603);
    assert!(
        lines
            .iter()
            .enumerate()
            .all(|(index, line)| line.new_line == Some(index as u32 + 1))
    );
}

#[test]
fn rejects_same_stat_content_changes() {
    let fixture = Fixture::new();
    fixture.write("file.rs", "original\n");
    fixture.commit();
    fixture.write("file.rs", "after1\n");
    let scope = fixture.working(500);
    let inspected = fixture.read(&scope, "file.rs", None, 0, 500).unwrap();
    let metadata = fs::metadata(fixture.root().join("file.rs")).unwrap();
    fixture.write("file.rs", "after2\n");
    let file = fs::OpenOptions::new()
        .write(true)
        .open(fixture.root().join("file.rs"))
        .unwrap();
    file.set_times(fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
        .unwrap();
    let repeated = list(
        &fixture.root(),
        None,
        Some(GitChangesCursor {
            snapshot: scope.snapshot.clone(),
            after_path: None,
        }),
        500,
        &fixture.control,
    )
    .unwrap();
    assert_eq!(repeated.snapshot, scope.snapshot);
    assert_eq!(
        fixture
            .read(&scope, "file.rs", Some(&inspected.revision), 0, 500)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn rejects_changed_working_metadata_and_index() {
    let fixture = Fixture::new();
    fixture.write("file.rs", "before\n");
    fixture.commit();
    fixture.write("file.rs", "after\n");
    let scope = fixture.working(500);
    fixture.write("another.rs", "new\n");
    assert_eq!(
        fixture
            .read(&scope, "file.rs", None, 0, 500)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let scope = fixture.working(500);
    let mut index = fixture.repository.index().unwrap();
    index.add_path(Path::new("file.rs")).unwrap();
    index.write().unwrap();
    assert_eq!(
        fixture
            .read(&scope, "file.rs", None, 0, 500)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[cfg(unix)]
#[test]
fn excludes_unchanged_index_entries_from_both_writers() {
    use std::os::unix::fs::MetadataExt;

    for cli in [false, true] {
        let fixture = Fixture::new();
        for name in [
            "README.md",
            "inventory/stock.py",
            "tests/test_stock.py",
            "data/sales.csv",
            "notes/meeting.md",
            "web/index.html",
        ] {
            fixture.write(name, "before\n");
        }
        if cli {
            fixture.commit_with_cli();
        } else {
            fixture.commit();
        }
        let index_path = fixture.repository.path().join("index");
        let index_bytes = fs::read(&index_path).unwrap();
        let index = git2::Index::open(&index_path).unwrap();
        let entry = index.get_path(Path::new("README.md"), 0).unwrap();
        let metadata = fs::metadata(fixture.root().join("README.md")).unwrap();
        assert_eq!(
            entry.dev,
            if cli { metadata.dev() } else { metadata.rdev() } as u32
        );
        assert_eq!(entry.ino, metadata.ino() as u32);

        let clean = fixture.working(500);
        assert!(clean.entries.is_empty(), "cli={cli}: {clean:?}");
        assert!(clean.exhausted);
        fixture.write("inventory/stock.py", "after\n");
        fixture.write("tests/test_stock.py", "after\n");
        let changed = fixture.working(500);
        assert_eq!(
            changed
                .entries
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            ["inventory/stock.py", "tests/test_stock.py"],
            "cli={cli}: {changed:?}"
        );
        assert!(changed.exhausted);
        assert_eq!(fs::read(&index_path).unwrap(), index_bytes);
        assert_eq!(
            list(
                &fixture.root(),
                None,
                Some(GitChangesCursor {
                    snapshot: clean.snapshot,
                    after_path: None,
                }),
                500,
                &fixture.control,
            )
            .unwrap_err()
            .code,
            ErrorCode::RevisionConflict
        );
    }
}

#[cfg(unix)]
#[test]
fn discovers_restored_mtime_changes() {
    let fixture = Fixture::new();
    fixture.write("file.rs", "before\n");
    fixture.commit();
    let clean = fixture.working(500);
    let index = fixture.repository.index().unwrap();
    let indexed = index.get_path(Path::new("file.rs"), 0).unwrap();
    assert!(
        clean.entries.is_empty(),
        "expected a clean index entry: stamp={:?}, index={indexed:?}, inventory={clean:?}",
        inventory::stamp(&fixture.root(), "file.rs").unwrap()
    );
    assert!(clean.exhausted);
    let destination = fixture.root().join("file.rs");
    let metadata = fs::metadata(&destination).unwrap();
    let original = inventory::stamp(&fixture.root(), "file.rs")
        .unwrap()
        .unwrap();
    fixture.write("file.rs", "after!\n");
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&destination)
        .unwrap();
    file.set_times(fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
        .unwrap();
    let changed = inventory::stamp(&fixture.root(), "file.rs")
        .unwrap()
        .unwrap();
    // Equal serialized stat data isolates ctime's role in discovering a new candidate.
    assert_eq!(
        serde_json::to_value(original).unwrap(),
        serde_json::to_value(changed).unwrap()
    );
    let scope = fixture.working(500);
    assert_eq!(scope.entries.len(), 1);
    assert_eq!(scope.entries[0].path, "file.rs");
    assert!(scope.exhausted);
    let content = fixture.read(&scope, "file.rs", None, 0, 500).unwrap();
    assert_eq!(
        content
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["before", "after!"]
    );
    assert_eq!(
        list(
            &fixture.root(),
            None,
            Some(GitChangesCursor {
                snapshot: clean.snapshot,
                after_path: None,
            }),
            500,
            &fixture.control
        )
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn skips_unreviewable_content_without_partial_lines() {
    let fixture = Fixture::new();
    fixture.write("binary.rs", [0, 1, 2]);
    fixture.write("invalid.rs", [255, 254]);
    fixture.write(
        "large-line.rs",
        format!("{}\n", "\"".repeat(MAX_DIFF_BYTES / 2)),
    );
    fixture.write("oversized.rs", "");
    fs::OpenOptions::new()
        .write(true)
        .open(fixture.root().join("oversized.rs"))
        .unwrap()
        .set_len(MAX_GIT_FILE_BYTES as u64 + 1)
        .unwrap();
    let scope = fixture.working(500);
    for (name, reason, binary) in [
        ("binary.rs", "binary", true),
        ("invalid.rs", "non_utf8", false),
        ("large-line.rs", "line_too_large", false),
        ("oversized.rs", "oversized", false),
    ] {
        let result = fixture.read(&scope, name, None, 0, 500).unwrap();
        assert_eq!(result.skip_reason.as_deref(), Some(reason));
        assert_eq!(result.binary, binary);
        assert!(result.lines.is_empty());
        assert!(result.next.is_none());
    }
}

#[cfg(unix)]
#[test]
fn does_not_follow_working_symlinks() {
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), "private bytes").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret"),
        fixture.root().join("link.rs"),
    )
    .unwrap();
    let scope = fixture.working(500);
    let result = fixture.read(&scope, "link.rs", None, 0, 100).unwrap();
    assert_eq!(result.skip_reason.as_deref(), Some("unsupported_mode"));
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("private bytes")
    );
}

#[test]
fn separates_index_and_working_sides() {
    let fixture = Fixture::new();
    fixture.write("file.rs", "committed\n");
    fixture.commit();
    fixture.write("file.rs", "staged\n");
    let mut index = fixture.repository.index().unwrap();
    index.add_path(Path::new("file.rs")).unwrap();
    index.write().unwrap();
    fixture.write("file.rs", "working\n");
    for (scope, before, after) in [
        (GitDiffScope::Staged, "committed", "staged"),
        (GitDiffScope::Unstaged, "staged", "working"),
        (GitDiffScope::All, "committed", "working"),
    ] {
        let inventory = list(
            &fixture.root(),
            Some(GitChangeTarget::Worktree { scope }),
            None,
            500,
            &fixture.control,
        )
        .unwrap();
        let content = fixture.read(&inventory, "file.rs", None, 0, 500).unwrap();
        assert_eq!(
            content
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            [before, after]
        );
    }
}

#[test]
fn marks_unsupported_paths_as_incomplete() {
    let fixture = Fixture::new();
    // Git trees store raw path bytes even when the host filesystem rejects them.
    let blob = fixture.repository.blob(b"private").unwrap();
    let mut builder = fixture.repository.treebuilder(None).unwrap();
    builder
        .insert([b'f', 255].as_slice(), blob, 0o100644)
        .unwrap();
    let tree = fixture
        .repository
        .find_tree(builder.write().unwrap())
        .unwrap();
    let author = Signature::now("Fixture", "fixture@example.test").unwrap();
    let commit = fixture
        .repository
        .commit(
            Some("HEAD"),
            &author,
            &author,
            "Invalid path fixture",
            &tree,
            &[],
        )
        .unwrap();
    let inventory = list(
        &fixture.root(),
        Some(GitChangeTarget::Commit {
            revision: commit.to_string(),
        }),
        None,
        500,
        &fixture.control,
    )
    .unwrap();
    assert!(inventory.entries.is_empty());
    assert!(inventory.next.is_none());
    assert!(!inventory.exhausted);
    assert_eq!(inventory.omitted_paths, 1);
}
