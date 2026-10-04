use super::*;
use sailry_protocol::*;

fn content(before: Option<&str>, after: &str) -> checkpoint::Content {
    let version = |text: &str| checkpoint::Version {
        revision: blake3::hash(text.as_bytes()).to_hex().to_string(),
        size: text.len() as u64,
    };
    checkpoint::Content {
        session: SessionId::new(),
        file: checkpoint::File {
            id: CheckpointId::new(),
            turn: TurnId::new(),
            worktree: WorktreeId::new(),
            path: "资料.txt".into(),
            before: before.map(version),
            after: version(after),
            outcome: RequestOutcome::Unknown,
        },
        before: before.map(str::to_owned),
        after: after.into(),
    }
}

#[test]
fn preserves_later_edits() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let original = "Original 中文 🙂\n".repeat(5000);
    let content = content(Some(&original), "after");
    let path = root.path().join(&content.file.path);
    for text in ["later", "after"] {
        std::fs::write(&path, text).unwrap();
        let result = restore(&root_path, &content);
        if text == "later" {
            assert_eq!(result.unwrap_err().code, ErrorCode::RevisionConflict);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "later");
        } else {
            let restored = result.unwrap();
            assert_eq!(
                restored.revision,
                Some(content.file.before.as_ref().unwrap().revision.clone())
            );
            assert_eq!(restored.session, content.session);
            assert_eq!(restored.checkpoint, content.file.id);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }
}

#[test]
fn protects_changed_creations() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let content = content(None, "after");
    let path = root.path().join(&content.file.path);
    for text in ["later".into(), "oversized".repeat(MAX_FILE_BYTES)] {
        std::fs::write(&path, &text).unwrap();
        assert_eq!(
            restore(&root_path, &content).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
    let other = tempfile::tempdir().unwrap();
    assert_eq!(
        restore(&other.path().canonicalize().unwrap(), &content)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[cfg(unix)]
#[test]
fn rejects_symlink_targets() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("target"), "after").unwrap();
    for before in [None, Some("before")] {
        let content = content(before, "after");
        let path = root.path().join(&content.file.path);
        if path.symlink_metadata().is_err() {
            std::os::unix::fs::symlink(outside.path().join("target"), &path).unwrap();
        }
        assert!(restore(&root_path, &content).is_err());
        assert!(path.symlink_metadata().unwrap().is_symlink());
        assert_eq!(
            std::fs::read_to_string(outside.path().join("target")).unwrap(),
            "after"
        );
    }
}
