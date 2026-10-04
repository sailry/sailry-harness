use super::*;
use std::fs;

mod relocate;

#[test]
fn confines_cross_root_copies() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let source = base.join("source");
    let target = base.join("target");
    fs::create_dir_all(source.join("folder/child")).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("folder/content"), "source bytes").unwrap();
    fs::write(target.join("content"), "other bytes").unwrap();
    between(&source, &target, "folder/content", "copied").unwrap();
    assert_eq!(
        fs::read_to_string(target.join("copied")).unwrap(),
        "source bytes"
    );
    between(&target, &source, "content", "copied").unwrap();
    assert_eq!(
        fs::read_to_string(source.join("copied")).unwrap(),
        "other bytes"
    );
    assert_eq!(
        between(&source, &source.join("folder/child"), "folder", "nested")
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert!(!source.join("folder/child/nested").exists());
}

#[test]
fn validates_roots_before_publication() {
    for replace_source in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let source = base.join("source");
        let target = base.join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("content"), "saved bytes").unwrap();
        let changed = if replace_source { &source } else { &target };
        let result = publish(&source, &target, "content", "copied", None, &|phase| {
            if matches!(phase, Phase::Prepared) {
                fs::rename(changed, base.join("retained")).unwrap();
                fs::create_dir(changed).unwrap();
                if replace_source {
                    fs::write(source.join("content"), "replacement").unwrap();
                }
            }
        });
        assert_eq!(result.unwrap_err().code, ErrorCode::RevisionConflict);
        assert!(!target.join("copied").exists());
        assert!(!base.join("retained/copied").exists());
        if !replace_source {
            assert_eq!(fs::read_dir(base.join("retained")).unwrap().count(), 0);
        }
    }
}

#[test]
fn copies_exclusively() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for (index, bytes) in [vec![0xff; 10 * 1024 * 1024], vec![]]
        .into_iter()
        .enumerate()
    {
        let source = format!("source-{index}");
        let target = format!("target-{index}");
        fs::write(root.join(&source), &bytes).unwrap();
        entry(&root, &source, &target).unwrap();
        assert_eq!(fs::read(root.join(&target)).unwrap(), bytes);
        assert_eq!(fs::read(root.join(&source)).unwrap(), bytes);
        fs::write(root.join(&target), b"keep").unwrap();
        assert_eq!(
            entry(&root, &source, &target).unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(fs::read(root.join(&target)).unwrap(), b"keep");
    }
    assert_eq!(fs::read_dir(root).unwrap().count(), 4);
}

#[test]
fn excludes_nested_git_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("资料/sub/empty")).unwrap();
    fs::create_dir_all(root.join("资料/.git")).unwrap();
    fs::write(root.join("资料/.git/config"), "private").unwrap();
    fs::write(root.join("资料/sub/.git"), "gitdir: elsewhere").unwrap();
    fs::write(root.join("资料/sub/文件.txt"), "contents").unwrap();
    entry(&root, "资料", "copied").unwrap();
    assert_eq!(
        fs::read_to_string(root.join("copied/sub/文件.txt")).unwrap(),
        "contents"
    );
    assert!(root.join("copied/sub/empty").is_dir());
    assert!(!root.join("copied/.git").exists());
    assert!(!root.join("copied/sub/.git").exists());
    assert!(root.join("资料/.git/config").is_file());
    for (from, to) in [
        ("资料", "资料/sub/nested"),
        ("资料", "资料/child"),
        ("资料", "资料"),
        ("资料", "../outside"),
        ("资料/.git", "metadata"),
        ("资料", ".git"),
        ("", "root"),
    ] {
        assert!(entry(&root, from, to).is_err());
    }
    if root.join("COPIED").exists() {
        assert!(entry(&root, "copied", "COPIED/alias").is_err());
    }
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn discards_conflicting_staging() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::write(root.join("source"), "contents").unwrap();
    let result = publish(&root, &root, "source", "target", None, &|phase| {
        if matches!(phase, Phase::Prepared) {
            fs::write(root.join("source"), "changed source").unwrap();
        }
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::RevisionConflict);
    assert!(!root.join("target").exists());
    let result = publish(&root, &root, "source", "target", None, &|phase| {
        if matches!(phase, Phase::Prepared) {
            fs::write(root.join("target"), "keep").unwrap();
        }
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::Conflict);
    assert_eq!(fs::read_to_string(root.join("target")).unwrap(), "keep");
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn uncertainty_preserves_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::write(root.join("source"), "contents").unwrap();
    let result = publish(&root, &root, "source", "target", None, &|phase| {
        if matches!(phase, Phase::Published) {
            fs::write(root.join("target"), "external edit").unwrap();
        }
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::OutcomeUnknown);
    assert_eq!(
        fs::read_to_string(root.join("target")).unwrap(),
        "external edit"
    );
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn bounds_tree_depth() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut folder = root.join("source");
    fs::create_dir(&folder).unwrap();
    for _ in 0..66 {
        folder.push("child");
        fs::create_dir(&folder).unwrap();
    }
    assert_eq!(
        entry(&root, "source", "target").unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert!(!root.join("target").exists());
    let source = path::root(&root).unwrap();
    let staging = TempDir::new_in(&source).unwrap();
    assert!(
        contents::Copier {
            count: 100_000,
            moving: false
        }
        .copy(&source, "source", &staging, "entry", 0)
        .is_err()
    );
    drop(staging);
    assert_eq!(fs::read_dir(root).unwrap().count(), 1);
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn validates_links_and_entry_types() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::write(root.join("outside"), "keep").unwrap();
        symlink("../outside", root.join("source/link")).unwrap();
        assert!(entry(&root, "source", "target").is_err());
        assert!(!root.join("target").exists());
        assert!(entry(&root, "source/link", "copied-link").is_err());
        symlink("source", root.join("parent")).unwrap();
        assert!(entry(&root, "outside", "parent/copied").is_err());
        let socket = std::os::unix::net::UnixListener::bind(root.join("socket")).unwrap();
        assert!(entry(&root, "socket", "copied-socket").is_err());
        drop(socket);
        fs::hard_link(root.join("outside"), root.join("hard-link")).unwrap();
        fs::set_permissions(root.join("hard-link"), fs::Permissions::from_mode(0o755)).unwrap();
        entry(&root, "hard-link", "copied").unwrap();
        assert_eq!(
            fs::metadata(root.join("copied"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        fs::write(root.join("copied"), "changed").unwrap();
        assert_eq!(fs::read_to_string(root.join("outside")).unwrap(), "keep");
        fs::set_permissions(root.join("hard-link"), fs::Permissions::from_mode(0o444)).unwrap();
        entry(&root, "hard-link", "readonly").unwrap();
        assert!(
            fs::metadata(root.join("readonly"))
                .unwrap()
                .permissions()
                .readonly()
        );
        fs::create_dir(root.join("private")).unwrap();
        fs::set_permissions(root.join("private"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("private/content"), "private directory content").unwrap();
        entry(&root, "private", "private-copy").unwrap();
        assert_eq!(
            fs::metadata(root.join("private-copy"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[test]
    fn rejects_invalidated_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir(root.join("folder")).unwrap();
        fs::write(root.join("source"), "contents").unwrap();
        let result = publish(&root, &root, "source", "folder/target", None, &|phase| {
            if matches!(phase, Phase::Prepared) {
                fs::rename(root.join("folder"), root.join("moved")).unwrap();
                fs::create_dir(root.join("folder")).unwrap();
            }
        });
        assert_eq!(result.unwrap_err().code, ErrorCode::RevisionConflict);
        assert_eq!(fs::read_dir(root.join("moved")).unwrap().count(), 0);
        assert!(!root.join("folder/target").exists());
        fs::write(root.join("folder/invalid\nname"), "bad").unwrap();
        assert!(entry(&root, "folder", "target").is_err());
        assert!(!root.join("target").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reports_cleanup_failure() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir(root.join("folder")).unwrap();
        fs::write(root.join("source"), "contents").unwrap();
        let result = publish(&root, &root, "source", "folder/target", None, &|phase| {
            if matches!(phase, Phase::Prepared) {
                fs::set_permissions(root.join("folder"), fs::Permissions::from_mode(0o500))
                    .unwrap();
            }
        });
        fs::set_permissions(root.join("folder"), fs::Permissions::from_mode(0o700)).unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains("temporary data remains"));
        assert!(!root.join("folder/target").exists());
        assert_eq!(fs::read_dir(root.join("folder")).unwrap().count(), 1);
        assert_eq!(fs::read_to_string(root.join("source")).unwrap(), "contents");
    }
}
