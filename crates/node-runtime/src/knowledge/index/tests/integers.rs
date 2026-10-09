use super::*;

#[test]
fn roundtrips_maximum_signed_values() {
    let (directory, mut index) = open();
    let maximum = u64::try_from(i64::MAX).unwrap();
    let binding = Binding {
        generation: maximum,
        rebuild: maximum,
        ..binding()
    };
    let mut source = document("note.md");
    source.modified_ms = maximum;
    index.bind(binding).unwrap();
    index
        .replace(binding, &source, &[part("向量", None)])
        .unwrap();
    drop(index);
    let mut index = Index::open(&directory.path().join("knowledge.db")).unwrap();
    index.bind(binding).unwrap();
    assert!(index.unchanged(binding, &source).unwrap());
    assert_eq!(index.counts(binding).unwrap(), (1, 1));
    let mut query = query();
    query.mode = Mode::Keyword;
    query.modified_after_ms = Some(maximum);
    query.modified_before_ms = Some(maximum);
    let page = index
        .search(&query, &[binding], &[], false, &CancellationToken::new())
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].citation.path, source.locator);
}

#[test]
fn rejects_overflow_without_replacing_sources() {
    let (_directory, mut index) = open();
    let original = binding();
    let source = document("note.md");
    index.bind(original).unwrap();
    index
        .replace(original, &source, &[part("向量", None)])
        .unwrap();
    let mut invalid = vec![
        Binding {
            generation: u64::MAX,
            ..original
        },
        Binding {
            rebuild: u64::MAX,
            ..original
        },
    ];
    if usize::BITS > 63 {
        invalid.push(Binding {
            model: Some((Model::BgeSmallZh, usize::MAX)),
            ..original
        });
    }
    for binding in invalid {
        assert_eq!(
            index.bind(binding).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            index.unchanged(binding, &source).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert!(index.unchanged(original, &source).unwrap());
        assert_eq!(index.counts(original).unwrap(), (1, 1));
    }
    let mut replacement = document("note.md");
    replacement.modified_ms = u64::MAX;
    replacement.revision = "replacement".into();
    assert_eq!(
        index
            .replace(original, &replacement, &[part("replacement", None)])
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert!(index.unchanged(original, &source).unwrap());
    assert_eq!(index.counts(original).unwrap(), (1, 1));
}

#[test]
fn rejects_out_of_range_query_filters() {
    let (_directory, mut index) = open();
    let binding = binding();
    index.bind(binding).unwrap();
    for before in [false, true] {
        let mut query = query();
        if before {
            query.modified_before_ms = Some(u64::MAX);
        } else {
            query.modified_after_ms = Some(u64::MAX);
        }
        assert_eq!(
            index
                .search(&query, &[binding], &[], false, &CancellationToken::new())
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    let invalid = Binding {
        generation: u64::MAX,
        ..binding
    };
    assert_eq!(
        index
            .search(&query(), &[invalid], &[], false, &CancellationToken::new())
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn rejects_negative_stored_metadata_without_writes() {
    let (_directory, mut index) = open();
    let binding = binding();
    let source = document("note.md");
    index.bind(binding).unwrap();
    index
        .replace(binding, &source, &[part("向量", None)])
        .unwrap();
    for (column, restored) in [("dimension", 2i64), ("rebuild", 0)] {
        index
            .db
            .execute(
                &format!("UPDATE collections SET {column}=-1 WHERE id=?1"),
                [binding.id.to_string()],
            )
            .unwrap();
        let fault = index.bind(binding).unwrap_err();
        assert_eq!(fault.code, ErrorCode::Internal);
        assert!(fault.message.contains("negative"));
        let stored: i64 = index
            .db
            .query_row(
                &format!("SELECT {column} FROM collections WHERE id=?1"),
                [binding.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, -1);
        index
            .db
            .execute(
                &format!("UPDATE collections SET {column}=?2 WHERE id=?1"),
                params![binding.id.to_string(), restored],
            )
            .unwrap();
        assert!(index.unchanged(binding, &source).unwrap());
        assert_eq!(index.counts(binding).unwrap(), (1, 1));
    }
}
