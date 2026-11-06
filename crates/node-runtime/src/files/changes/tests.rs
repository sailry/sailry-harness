use super::*;
use sailry_protocol::{ChangesStatus, FileEdit};
use std::path::Path;

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

fn root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "fn old() {}\n").unwrap();
    std::fs::write(root.join("README.md"), "# Original\n").unwrap();
    std::fs::write(root.join("src/old.rs"), "moved\n").unwrap();
    std::fs::write(root.join("src/dead.rs"), "dead\n").unwrap();
    (directory, root)
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for entry in walkdir(root) {
        files.push((
            entry
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            std::fs::read(&entry).unwrap(),
        ));
    }
    files.sort();
    files
}

fn walkdir(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walkdir(&path));
        } else {
            files.push(path);
        }
    }
    files
}

fn edit(path: &str, before: &str, old_text: &str, new_text: &str) -> FileChange {
    FileChange::Edit {
        path: path.into(),
        expected_revision: revision(before),
        edits: vec![FileEdit {
            old_text: old_text.into(),
            new_text: new_text.into(),
            replace_all: false,
        }],
    }
}

fn create(path: &str, text: &str) -> FileChange {
    FileChange::Create {
        path: path.into(),
        text: text.into(),
    }
}

fn run(root: &Path, changes: &[FileChange]) -> FileChanges {
    let scope = super::super::Access::Restricted.scope(root);
    match plan(scope, changes, &[], &CancellationToken::new()) {
        Ok(planned) => apply(scope, changes, planned),
        Err(rejected) => rejected,
    }
}

mod validation {
    use super::*;

    #[test]
    fn rejects_each_reason_without_writing() {
        let (_directory, root) = root();
        let valid = edit("src/lib.rs", "fn old() {}\n", "old", "new");
        let stale = revision("stale");
        for (case, change, code) in [
            (
                "existing create target",
                create("README.md", "replaced"),
                ErrorCode::Conflict,
            ),
            (
                "stale write",
                FileChange::Write {
                    path: "README.md".into(),
                    text: "replaced".into(),
                    expected_revision: stale.clone(),
                },
                ErrorCode::RevisionConflict,
            ),
            (
                "missing edit text",
                edit("README.md", "# Original\n", "absent", "x"),
                ErrorCode::InvalidRequest,
            ),
            (
                "stale edit",
                edit("README.md", "changed", "Original", "x"),
                ErrorCode::RevisionConflict,
            ),
            (
                "existing move destination",
                FileChange::Move {
                    path: "src/old.rs".into(),
                    destination: "README.md".into(),
                    expected_revision: revision("moved\n"),
                },
                ErrorCode::Conflict,
            ),
            (
                "missing move parent",
                FileChange::Move {
                    path: "src/old.rs".into(),
                    destination: "missing/new.rs".into(),
                    expected_revision: revision("moved\n"),
                },
                ErrorCode::NotFound,
            ),
            (
                "stale delete",
                FileChange::Delete {
                    path: "src/dead.rs".into(),
                    expected_revision: stale.clone(),
                },
                ErrorCode::RevisionConflict,
            ),
            (
                "malformed revision",
                FileChange::Delete {
                    path: "src/dead.rs".into(),
                    expected_revision: "head".into(),
                },
                ErrorCode::InvalidRequest,
            ),
            (
                "oversized create",
                create("big.txt", &"x".repeat(MAX_FILE_BYTES + 1)),
                ErrorCode::InvalidRequest,
            ),
            (
                "NUL create",
                create("nul.txt", "a\0b"),
                ErrorCode::InvalidRequest,
            ),
            (
                "duplicate path",
                edit("src/lib.rs", "fn old() {}\n", "fn", "pub fn"),
                ErrorCode::InvalidRequest,
            ),
            (
                "case alias",
                create("SRC/LIB.rs", "alias"),
                ErrorCode::InvalidRequest,
            ),
        ] {
            let before = snapshot(&root);
            let result = run(&root, &[valid.clone(), change]);
            assert_eq!(result.status, ChangesStatus::Rejected, "{case}");
            assert_eq!(result.files[0].status, ChangeStatus::NotAttempted, "{case}");
            assert_eq!(result.files[1].status, ChangeStatus::Failed, "{case}");
            assert_eq!(
                result.files[1].error.as_ref().map(|error| error.code),
                Some(code),
                "{case}: {:?}",
                result.files[1].error
            );
            assert_eq!(snapshot(&root), before, "{case}");
        }
    }

    #[test]
    fn rejects_protected_and_unconfined_paths() {
        let (_directory, root) = root();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), "[core]\n").unwrap();
        for (path, code) in [
            (".git/hooks/pre-commit", ErrorCode::PermissionDenied),
            ("nested/.GIT/config", ErrorCode::PermissionDenied),
            (".sailry/worktrees/child/file", ErrorCode::PermissionDenied),
            ("../outside.txt", ErrorCode::InvalidRequest),
            ("/absolute.txt", ErrorCode::InvalidRequest),
        ] {
            let before = snapshot(&root);
            let result = run(&root, &[create("allowed.txt", "x"), create(path, "x")]);
            assert_eq!(result.status, ChangesStatus::Rejected, "{path}");
            assert_eq!(
                result.files[1].error.as_ref().map(|error| error.code),
                Some(code),
                "{path}"
            );
            assert_eq!(snapshot(&root), before, "{path}");
        }
        let moved = FileChange::Move {
            path: "src/old.rs".into(),
            destination: ".git/old.rs".into(),
            expected_revision: revision("moved\n"),
        };
        let result = run(&root, &[moved]);
        assert_eq!(
            result.files[0].error.as_ref().map(|error| error.code),
            Some(ErrorCode::PermissionDenied)
        );
        assert!(root.join("src/old.rs").exists());
    }

    #[test]
    fn reports_node_resource_checks_per_entry() {
        let (_directory, root) = root();
        let changes = [create("first.txt", "1"), create("second.txt", "2")];
        let denied = Fault::new(
            ErrorCode::Conflict,
            "entry contains registered resources or Node storage",
        );
        let Err(rejected) = plan(
            super::super::super::Access::Restricted.scope(&root),
            &changes,
            &[None, Some(denied.clone())],
            &CancellationToken::new(),
        ) else {
            panic!("rejection expected")
        };
        assert_eq!(rejected.status, ChangesStatus::Rejected);
        assert_eq!(rejected.files[0].status, ChangeStatus::NotAttempted);
        assert_eq!(rejected.files[1].error, Some(denied));
        assert!(!root.join("first.txt").exists());
    }

    #[test]
    fn bounds_entry_count() {
        assert!(shape(&[]).is_err());
        let many: Vec<_> = (0..=MAX_CHANGES)
            .map(|index| create(&format!("{index}.txt"), ""))
            .collect();
        assert_eq!(shape(&many).unwrap_err().code, ErrorCode::InvalidRequest);
        assert!(shape(&many[..MAX_CHANGES]).is_ok());
    }
}

mod writing {
    use super::*;

    #[test]
    fn applies_entries_in_order() {
        let (_directory, root) = root();
        let changes = [
            create("src/nested/new.rs", "pub fn new() {}\n"),
            FileChange::Write {
                path: "README.md".into(),
                text: "# Replaced\n".into(),
                expected_revision: revision("# Original\n"),
            },
            edit("src/lib.rs", "fn old() {}\n", "old", "renamed"),
            FileChange::Move {
                path: "src/old.rs".into(),
                destination: "src/moved.rs".into(),
                expected_revision: revision("moved\n"),
            },
        ];
        let result = run(&root, &changes);
        assert_eq!(result.status, ChangesStatus::Applied, "{result:?}");
        assert!(
            result
                .files
                .iter()
                .all(|file| file.status == ChangeStatus::Applied && file.error.is_none())
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/nested/new.rs")).unwrap(),
            "pub fn new() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "# Replaced\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "fn renamed() {}\n"
        );
        assert!(!root.join("src/old.rs").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("src/moved.rs")).unwrap(),
            "moved\n"
        );
        assert_eq!(
            result.files[2].revision.as_deref(),
            Some(revision("fn renamed() {}\n").as_str())
        );
        assert_eq!(result.files[2].size, Some(16));
        let excerpt = result.files[2].excerpt.as_ref().unwrap();
        assert_eq!(excerpt.hunks[0].lines[0].text, "fn renamed() {}\n");
        assert!(result.files[1].excerpt.is_none());
        assert_eq!(result.files[3].destination.as_deref(), Some("src/moved.rs"));
        assert_eq!(
            result.files[3].revision.as_deref(),
            Some(revision("moved\n").as_str())
        );
    }

    #[test]
    fn stops_at_a_rechecked_revision() {
        let (_directory, root) = root();
        let changes = [
            create("first.txt", "first\n"),
            edit("src/lib.rs", "fn old() {}\n", "old", "new"),
            FileChange::Write {
                path: "README.md".into(),
                text: "# Never\n".into(),
                expected_revision: revision("# Original\n"),
            },
        ];
        let scope = super::super::super::Access::Restricted.scope(&root);
        let planned = plan(scope, &changes, &[], &CancellationToken::new()).unwrap();
        // An external write between validation and replacement must be detected.
        std::fs::write(root.join("src/lib.rs"), "fn external() {}\n").unwrap();
        let result = apply(scope, &changes, planned);
        assert_eq!(result.status, ChangesStatus::Partial);
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.status)
                .collect::<Vec<_>>(),
            [
                ChangeStatus::Applied,
                ChangeStatus::Failed,
                ChangeStatus::NotAttempted
            ]
        );
        assert_eq!(
            result.files[1].error.as_ref().map(|error| error.code),
            Some(ErrorCode::RevisionConflict)
        );
        assert_eq!(
            std::fs::read_to_string(root.join("first.txt")).unwrap(),
            "first\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "fn external() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "# Original\n"
        );
    }

    #[test]
    fn first_failure_writes_nothing() {
        let (_directory, root) = root();
        let changes = [
            create("created.txt", "created\n"),
            create("second.txt", "second\n"),
        ];
        let scope = super::super::super::Access::Restricted.scope(&root);
        let planned = plan(scope, &changes, &[], &CancellationToken::new()).unwrap();
        std::fs::write(root.join("created.txt"), "raced\n").unwrap();
        let result = apply(scope, &changes, planned);
        assert_eq!(result.status, ChangesStatus::Rejected);
        assert_eq!(result.files[0].status, ChangeStatus::Failed);
        assert_eq!(result.files[1].status, ChangeStatus::NotAttempted);
        assert_eq!(
            std::fs::read_to_string(root.join("created.txt")).unwrap(),
            "raced\n"
        );
        assert!(!root.join("second.txt").exists());
    }
}
