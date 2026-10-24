use super::*;
use crate::knowledge::source::Stat;

fn stamp() -> Stat {
    Stat {
        size: 120,
        modified_seconds: 1_700_000_000,
        modified_nanos: 123,
        extractor: [0; 32],
    }
}

#[tokio::test]
async fn checks_exact_witness_and_processed_model() {
    let (_directory, index) = open().await;
    let selected = binding();
    index.bind(&selected).await.unwrap();
    let mut document = document("notes.md");
    document.embedded = false;
    index
        .replace(&selected, &document, &[part("cached content", None)])
        .await
        .unwrap();
    let stat = stamp();
    assert!(
        !index
            .cached(&selected, &document.locator, stat, &document.tags, false)
            .await
            .unwrap()
    );
    index
        .remember(&selected, &document.locator, stat)
        .await
        .unwrap();
    assert!(
        index
            .cached(&selected, &document.locator, stat, &document.tags, false)
            .await
            .unwrap()
    );
    assert!(
        !index
            .cached(&selected, &document.locator, stat, &document.tags, true)
            .await
            .unwrap()
    );
    for changed in [
        Stat {
            size: stat.size + 1,
            ..stat
        },
        Stat {
            modified_seconds: stat.modified_seconds + 1,
            ..stat
        },
        Stat {
            modified_nanos: stat.modified_nanos + 1,
            ..stat
        },
        Stat {
            extractor: [1; 32],
            ..stat
        },
    ] {
        assert!(
            !index
                .cached(&selected, &document.locator, changed, &document.tags, false)
                .await
                .unwrap()
        );
    }
    assert!(
        !index
            .cached(&selected, "other.md", stat, &document.tags, false)
            .await
            .unwrap()
    );
    assert!(
        !index
            .cached(&selected, &document.locator, stat, &["other".into()], false)
            .await
            .unwrap()
    );
    let changed = Binding {
        generation: 2,
        ..selected.clone()
    };
    assert!(
        !index
            .cached(&changed, &document.locator, stat, &document.tags, false)
            .await
            .unwrap()
    );
    assert_eq!(
        index
            .remember(&changed, &document.locator, stat)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    index.bind(&changed).await.unwrap();
    assert!(
        index
            .cached(&changed, &document.locator, stat, &document.tags, false)
            .await
            .unwrap()
    );
    document.embedded = true;
    index
        .replace(
            &changed,
            &document,
            &[part("cached content", Some(&[1., 0.]))],
        )
        .await
        .unwrap();
    assert!(
        !index
            .cached(&changed, &document.locator, stat, &document.tags, true)
            .await
            .unwrap()
    );
    index
        .remember(&changed, &document.locator, stat)
        .await
        .unwrap();
    assert!(
        index
            .cached(&changed, &document.locator, stat, &document.tags, true)
            .await
            .unwrap()
    );
    let model = Binding {
        generation: 3,
        model: Some(("granite-multi".to_owned(), 3)),
        ..changed.clone()
    };
    index.bind(&model).await.unwrap();
    assert!(
        !index
            .cached(&model, &document.locator, stat, &document.tags, true)
            .await
            .unwrap()
    );
    assert!(
        index
            .cached(&model, &document.locator, stat, &document.tags, false)
            .await
            .unwrap()
    );
    index.prune(&model, &[]).await.unwrap();
    let lease = index.lease(true).await.unwrap();
    assert_eq!(
        lease
            .connection()
            .prepare("SELECT count(*) FROM file_stamps")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn fills_missing_derived_table_without_losing_sources() {
    let (directory, index) = open().await;
    let selected = binding();
    index.bind(&selected).await.unwrap();
    let document = document("notes.md");
    index
        .replace(
            &selected,
            &document,
            &[part("cached content", Some(&[1., 0.]))],
        )
        .await
        .unwrap();
    index
        .lease(false)
        .await
        .unwrap()
        .connection()
        .execute_batch("DROP TABLE file_stamps")
        .await
        .unwrap();
    drop(index);
    let index = Index::open(&directory.path().join("knowledge.turso"))
        .await
        .unwrap();
    assert_eq!(index.counts(&selected).await.unwrap(), (1, 1));
    assert!(index.unchanged(&selected, &document).await.unwrap());
    assert!(
        !index
            .cached(&selected, &document.locator, stamp(), &document.tags, true)
            .await
            .unwrap()
    );
    index
        .remember(&selected, &document.locator, stamp())
        .await
        .unwrap();
    assert!(
        index
            .cached(&selected, &document.locator, stamp(), &document.tags, true)
            .await
            .unwrap()
    );
    let lease = index.lease(true).await.unwrap();
    assert_eq!(
        lease
            .connection()
            .prepare("SELECT version FROM metadata")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        1
    );
}
