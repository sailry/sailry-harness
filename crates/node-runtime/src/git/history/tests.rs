use super::*;
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

thread_local! {
    pub(super) static VISITED: Cell<usize> = const { Cell::new(0) };
}

#[test]
fn bounds_utf8_text() {
    let (text, truncated) = bounded_text("中文提交".as_bytes(), 7);
    assert_eq!(text, "中文");
    assert!(truncated);
    let (text, truncated) = bounded_text(&[0xff; 20], 8);
    assert!(text.len() <= 8 && truncated);
    assert_eq!(bounded_text(b"commit", 8), ("commit".into(), false));
}

#[test]
fn bounds_filtered_traversal() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let repository = Repository::init(&root).unwrap();
    let mut parent = None;
    let mut target = repository.blob(b"initial").unwrap();
    for index in 0..120 {
        if index % 3 == 2 {
            target = repository.blob(index.to_string().as_bytes()).unwrap();
        }
        let mut tree = repository.treebuilder(None).unwrap();
        tree.insert("target.rs", target, 0o100644).unwrap();
        let other = repository.blob(index.to_string().as_bytes()).unwrap();
        tree.insert("other.rs", other, 0o100644).unwrap();
        let tree = repository.find_tree(tree.write().unwrap()).unwrap();
        let time = git2::Time::new(1000 + index, 0);
        let signature = git2::Signature::new("Fixture", "fixture@example.invalid", &time).unwrap();
        let parents = parent
            .map(|parent| repository.find_commit(parent).unwrap())
            .into_iter()
            .collect::<Vec<_>>();
        parent = Some(
            repository
                .commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    "Change",
                    &tree,
                    &parents.iter().collect::<Vec<_>>(),
                )
                .unwrap(),
        );
    }
    let control = Control {
        cancelled: sailry_link::CancellationToken::new(),
        deadline: Instant::now() + Duration::from_secs(5),
    };
    VISITED.set(0);
    let first = log(&root, 2, None, None, Some("target.rs"), &control).unwrap();
    assert_eq!(first.entries.len(), 2);
    assert_eq!(VISITED.get(), 7);
    assert!(first.truncated);
    assert_eq!(
        log(
            &root,
            2,
            first.next.as_ref(),
            None,
            Some("other.rs"),
            &control
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        log(&root, 2, first.next.as_ref(), None, None, &control)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let next = log(
        &root,
        2,
        first.next.as_ref(),
        None,
        Some("target.rs"),
        &control,
    )
    .unwrap();
    assert_eq!(next.offset, 2);
    assert_eq!(next.entries.len(), 2);
    assert!(
        next.entries
            .iter()
            .all(|entry| first.entries.iter().all(|previous| entry.id != previous.id))
    );
}
