use super::*;

#[tokio::test]
async fn roundtrips_maximum_signed_values() {
    let (directory, index) = open().await;
    let maximum = u64::try_from(i64::MAX).unwrap();
    let binding = Binding {
        generation: maximum,
        rebuild: maximum,
        ..binding()
    };
    let mut source = document("note.md");
    source.modified_ms = maximum;
    index.bind(&binding).await.unwrap();
    index
        .replace(&binding, &source, &[part("向量", None)])
        .await
        .unwrap();
    drop(index);
    let index = Index::open(&directory.path().join("knowledge.turso"))
        .await
        .unwrap();
    index.bind(&binding).await.unwrap();
    assert!(index.unchanged(&binding, &source).await.unwrap());
    assert_eq!(index.counts(&binding).await.unwrap(), (1, 1));
    let mut query = query();
    query.mode = Mode::Keyword;
    query.modified_after_ms = Some(maximum);
    query.modified_before_ms = Some(maximum);
    let page = index
        .search(
            &query,
            std::slice::from_ref(&binding),
            &[],
            false,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].citation.path, source.locator);
}

#[tokio::test]
async fn rejects_overflow_without_replacing_sources() {
    let (_directory, index) = open().await;
    let original = binding();
    let source = document("note.md");
    index.bind(&original).await.unwrap();
    index
        .replace(&original, &source, &[part("向量", None)])
        .await
        .unwrap();
    let mut invalid = vec![
        Binding {
            generation: u64::MAX,
            ..original.clone()
        },
        Binding {
            rebuild: u64::MAX,
            ..original.clone()
        },
    ];
    if usize::BITS > 63 {
        invalid.push(Binding {
            model: Some(("bge-small-zh".to_owned(), usize::MAX)),
            ..original.clone()
        });
    }
    for binding in invalid {
        assert_eq!(
            index.bind(&binding).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            index.unchanged(&binding, &source).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert!(index.unchanged(&original, &source).await.unwrap());
        assert_eq!(index.counts(&original).await.unwrap(), (1, 1));
    }
    let mut replacement = document("note.md");
    replacement.modified_ms = u64::MAX;
    replacement.revision = "replacement".into();
    assert_eq!(
        index
            .replace(&original, &replacement, &[part("replacement", None)])
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert!(index.unchanged(&original, &source).await.unwrap());
    assert_eq!(index.counts(&original).await.unwrap(), (1, 1));
}

#[tokio::test]
async fn rejects_out_of_range_query_filters() {
    let (_directory, index) = open().await;
    let binding = binding();
    index.bind(&binding).await.unwrap();
    for before in [false, true] {
        let mut query = query();
        if before {
            query.modified_before_ms = Some(u64::MAX);
        } else {
            query.modified_after_ms = Some(u64::MAX);
        }
        assert_eq!(
            index
                .search(
                    &query,
                    std::slice::from_ref(&binding),
                    &[],
                    false,
                    &CancellationToken::new()
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    let invalid = Binding {
        generation: u64::MAX,
        ..binding.clone()
    };
    assert_eq!(
        index
            .search(&query(), &[invalid], &[], false, &CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn rejects_negative_stored_metadata_without_writes() {
    let (_directory, index) = open().await;
    let binding = binding();
    let source = document("note.md");
    index.bind(&binding).await.unwrap();
    index
        .replace(&binding, &source, &[part("向量", None)])
        .await
        .unwrap();
    let lease = index.lease(false).await.unwrap();
    let db = lease.connection();
    for (column, restored) in [("dimension", 2i64), ("rebuild", 0)] {
        db.execute(
            &format!("UPDATE collections SET {column}=-1 WHERE id=?1"),
            [binding.id.to_string()],
        )
        .await
        .unwrap();
        let fault = index.bind(&binding).await.unwrap_err();
        assert_eq!(fault.code, ErrorCode::Internal);
        assert!(fault.message.contains("negative"));
        let stored: i64 = db
            .prepare(format!("SELECT {column} FROM collections WHERE id=?1"))
            .await
            .unwrap()
            .query_row([binding.id.to_string()])
            .await
            .unwrap()
            .get(0)
            .unwrap();
        assert_eq!(stored, -1);
        db.execute(
            &format!("UPDATE collections SET {column}=?2 WHERE id=?1"),
            params![binding.id.to_string(), restored],
        )
        .await
        .unwrap();
        assert!(index.unchanged(&binding, &source).await.unwrap());
        assert_eq!(index.counts(&binding).await.unwrap(), (1, 1));
    }
}
