use super::*;
use sailry_protocol::{DirectoryCursor, EntryKind, FileEntry};

fn page(start: usize, count: usize, more: bool) -> Directory {
    let entries: Vec<_> = (start..start + count)
        .map(|index| FileEntry {
            name: format!("{index:04}"),
            kind: EntryKind::File,
            size: 0,
        })
        .collect();
    Directory {
        path: "folder".into(),
        revision: "fixture".into(),
        next: more.then(|| DirectoryCursor {
            revision: "fixture".into(),
            directory: false,
            name: entries.last().unwrap().name.clone(),
        }),
        entries,
        truncated: more,
        unsupported_names: 0,
    }
}

#[test]
fn appends_only_matching_ordered_pages() {
    let first = page(0, MAX_DIRECTORY_ENTRIES, true);
    let combined = append("folder", Some(&first), page(500, 300, false)).unwrap();
    assert_eq!(combined.entries.len(), 800);
    assert_eq!(first.entries.len(), 500);
    assert!(!combined.truncated);
    assert!(combined.next.is_none());
    let mut changed = page(500, 300, false);
    changed.revision = "changed".into();
    assert_eq!(
        append("folder", Some(&first), changed).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        append("folder", Some(&first), page(499, 300, false))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        append("different", None, page(0, 300, false))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn rejects_invalid_continuations() {
    let mut missing = page(0, 3, false);
    missing.revision.clear();
    assert!(append("folder", None, missing).is_err());
    for mut invalid in [page(0, 3, true), page(0, 501, false), page(0, 500, true)] {
        if let Some(cursor) = invalid.next.as_mut() {
            cursor.name = "incorrect".into();
        }
        assert!(append("folder", None, invalid).is_err());
    }
    let mut duplicate = page(0, 3, false);
    duplicate.entries[0].kind = EntryKind::Directory;
    duplicate.entries[1].name = duplicate.entries[0].name.clone();
    assert!(append("folder", None, duplicate).is_err());
    let mut unsorted = page(0, 3, false);
    unsorted.entries.swap(0, 2);
    assert!(append("folder", None, unsorted).is_err());
    assert!(append("folder", None, page(0, 0, false)).is_ok());
}
