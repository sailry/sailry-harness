use super::*;
use sailry_protocol::{Command, NodeId, RequestId, TurnId, plugin};

#[test]
fn excludes_internal_and_captured_calls() {
    let mut request = Request::new(NodeId([1; 32]), Command::ListPlugins);
    assert_eq!(Access::request(&request, false), Access::Manual);
    assert_eq!(Access::request(&request, true), Access::Restricted);
    let mut context = plugin::Context {
        package: plugin::Reference {
            name: "fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        worktree: None,
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    };
    request.plugin = Some(context.clone());
    assert!(Access::request(&request, false) == Access::Manual);
    context.turn = Some(TurnId::new());
    request.plugin = Some(context.clone());
    assert!(Access::request(&request, false) == Access::Restricted);
    context.turn = None;
    context.invocation = Some(RequestId::new());
    request.plugin = Some(context);
    assert!(Access::request(&request, false) == Access::Restricted);
}

#[test]
fn confines_paths_in_both_modes() {
    let directory = tempfile::tempdir().unwrap();
    let manual = Access::Manual.scope(directory.path());
    let restricted = Access::Restricted.scope(directory.path());
    for path in [
        ".sailry/worktrees",
        ".sailry/worktrees/id/file",
        ".SAILRY/WORKTREES/id/file",
    ] {
        assert!(manual.components(path, false).is_ok());
        assert_eq!(
            restricted.components(path, false).unwrap_err().code,
            sailry_protocol::ErrorCode::PermissionDenied
        );
    }
    for path in [
        "../file",
        "/absolute",
        "folder/../file",
        "folder//file",
        "folder\\file",
    ] {
        assert!(manual.components(path, false).is_err());
        assert!(restricted.components(path, false).is_err());
    }
    for path in [
        ".git/index",
        ".sailry/worktrees/id/.git",
        ".sailry/worktrees/id/.GIT/config",
    ] {
        assert_eq!(
            manual.entries(path).unwrap_err().code,
            sailry_protocol::ErrorCode::PermissionDenied
        );
    }
}

#[test]
fn retains_manual_scope_through_kernels() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".sailry/worktrees/id")).unwrap();
    let scope = Access::Manual.scope(&root);
    let base = ".sailry/worktrees/id";
    super::super::directory(scope, &format!("{base}/folder")).unwrap();
    let source = format!("{base}/folder/file.txt");
    let created = super::super::save(scope, &source, "original", None).unwrap();
    assert_eq!(
        super::super::save(&root, &source, "denied", Some(&created.revision))
            .unwrap_err()
            .code,
        sailry_protocol::ErrorCode::PermissionDenied
    );
    let updated = super::super::save(scope, &source, "updated", Some(&created.revision)).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(&source)).unwrap(),
        "updated"
    );
    assert_eq!(
        super::super::save(scope, &source, "conflict", Some(&created.revision))
            .unwrap_err()
            .code,
        sailry_protocol::ErrorCode::RevisionConflict
    );
    let copied = format!("{base}/copied.txt");
    super::super::copy::entry(scope, &source, &copied).unwrap();
    let moved = format!("{base}/renamed.txt");
    super::super::rename(scope, &copied, &moved).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(&moved)).unwrap(),
        "updated"
    );
    assert_eq!(
        super::super::version::current(scope, &source)
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        updated.revision,
        blake3::hash(b"updated").to_hex().to_string()
    );
}

#[tokio::test]
async fn separates_discovery_and_inspection_scopes() {
    use sailry_protocol::{ErrorCode, FileSearch, Output, SearchMode, WorktreeId};
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let base = ".sailry/worktrees/id";
    std::fs::create_dir_all(root.join(base).join(".git")).unwrap();
    std::fs::write(root.join(base).join("visible.txt"), "managed needle").unwrap();
    std::fs::write(root.join(base).join("ignored.txt"), "managed needle").unwrap();
    std::fs::write(root.join(base).join(".git/config"), "managed needle").unwrap();
    std::fs::write(root.join(base).join(".gitignore"), "ignored.txt\n").unwrap();
    let files = super::super::Files::new();
    let worktree = WorktreeId::new();
    for access in [Access::Restricted, Access::Manual] {
        let result = files
            .inspect_with(
                root.clone(),
                Command::ReadFile {
                    worktree,
                    path: format!("{base}/visible.txt"),
                },
                sailry_link::CancellationToken::new(),
                access,
            )
            .await;
        if access == Access::Manual {
            assert!(
                matches!(result, Ok(Output::FileContent(ref content)) if content.text == "managed needle")
            );
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::PermissionDenied);
        }
        let Output::FileNames(names) = files
            .inspect_with(
                root.clone(),
                Command::FindFiles {
                    worktree,
                    globs: vec!["**/*.txt".into()],
                    path: String::new(),
                    limit: 500,
                    offset: 0,
                },
                sailry_link::CancellationToken::new(),
                access,
            )
            .await
            .unwrap()
        else {
            panic!("file names expected")
        };
        let expected = if access == Access::Manual {
            vec![format!("{base}/visible.txt")]
        } else {
            Vec::new()
        };
        assert_eq!(names.paths, expected);
        let Output::SearchResults(found) = files
            .inspect_with(
                root.clone(),
                Command::SearchFiles {
                    worktree,
                    options: FileSearch {
                        query: "managed needle".into(),
                        mode: SearchMode::Files,
                        ..Default::default()
                    },
                },
                sailry_link::CancellationToken::new(),
                access,
            )
            .await
            .unwrap()
        else {
            panic!("search results expected")
        };
        assert_eq!(found.paths, expected);
    }
}

#[cfg(unix)]
#[test]
fn retains_parent_and_revision_checks() {
    use super::super::save::Target;
    use sailry_protocol::ErrorCode;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let base = ".sailry/worktrees/id";
    std::fs::create_dir_all(root.join(base)).unwrap();
    let path = format!("{base}/file.txt");
    let scope = Access::Manual.scope(&root);
    std::fs::write(root.join(&path), "original").unwrap();
    let revision = blake3::hash(b"original").to_hex().to_string();
    assert!(
        matches!(Target::prepare(&root, &path, Some(&revision), u64::MAX, None), Err(ref error) if error.code == ErrorCode::PermissionDenied)
    );
    let target = Target::prepare(scope, &path, Some(&revision), u64::MAX, None).unwrap();
    std::fs::rename(root.join(base), root.join("retained")).unwrap();
    std::fs::write(outside.path().join("file.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join(base)).unwrap();
    assert!(
        target
            .write(
                &mut &b"ours"[..],
                4,
                blake3::hash(b"ours").to_hex().as_ref(),
                &|_| {}
            )
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("file.txt")).unwrap(),
        "outside"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("retained/file.txt")).unwrap(),
        "original"
    );
}
