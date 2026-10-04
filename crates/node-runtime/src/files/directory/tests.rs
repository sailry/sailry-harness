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
fn preserves_global_order() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for index in (0..1200).rev() {
        std::fs::write(root.join(format!("{index:04}.txt")), "fixture").unwrap();
    }
    for name in ["资料", "folder-z", "folder-a"] {
        std::fs::create_dir(root.join(name)).unwrap();
    }
    let mut names = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = list(&root, String::new(), cursor.as_ref(), &control()).unwrap();
        assert!(page.entries.len() <= MAX_DIRECTORY_ENTRIES);
        assert_eq!(page.truncated, page.next.is_some());
        names.extend(page.entries.into_iter().map(|entry| entry.name));
        cursor = page.next;
        pages += 1;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(&names[..3], ["folder-a", "folder-z", "资料"]);
    let expected: Vec<_> = (0..1200).map(|index| format!("{index:04}.txt")).collect();
    assert_eq!(&names[3..], expected);
}

#[test]
fn validates_cursor_identity() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("folder")).unwrap();
    for index in 0..501 {
        std::fs::write(root.join("folder").join(format!("{index:04}")), "").unwrap();
    }
    let first = list(&root, "folder".into(), None, &control()).unwrap();
    let cursor = first.next.unwrap();
    assert_eq!(
        list(&root, "".into(), Some(&cursor), &control())
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    std::fs::rename(root.join("folder"), root.join("old")).unwrap();
    std::fs::create_dir(root.join("folder")).unwrap();
    assert_eq!(
        list(&root, "folder".into(), Some(&cursor), &control())
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    for name in ["", "../escape", "file/name", "file\0name"] {
        let cursor = DirectoryCursor {
            name: name.into(),
            ..cursor.clone()
        };
        assert_eq!(
            list(&root, "folder".into(), Some(&cursor), &control())
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    let changed = list(&root, "old".into(), None, &control())
        .unwrap()
        .next
        .unwrap();
    // In-place entry changes invalidate the ordinary directory metadata revision.
    for index in 501..550 {
        std::fs::write(root.join("old").join(format!("{index:04}")), "").unwrap();
    }
    assert_eq!(
        list(&root, "old".into(), Some(&changed), &control())
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn skips_unsupported_names_and_cancels() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    // Unicode names are kept, while protocol-invalid control characters are not aliased.
    #[cfg(unix)]
    std::fs::write(root.join("line\nbreak"), "").unwrap();
    std::fs::write(root.join("文件.txt"), "").unwrap();
    let page = list(&root, String::new(), None, &control()).unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "文件.txt");
    #[cfg(unix)]
    assert_eq!(page.unsupported_names, 1);
    let cancelled = control();
    cancelled.cancelled.cancel();
    assert_eq!(
        list(&root, String::new(), None, &cancelled)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    let expired = Control {
        deadline: Instant::now(),
        ..control()
    };
    assert_eq!(
        list(&root, String::new(), None, &expired).unwrap_err().code,
        ErrorCode::Busy
    );
}
