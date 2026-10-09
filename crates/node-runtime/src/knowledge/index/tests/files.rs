use super::*;
use crate::knowledge::source::Stat;

fn stamp() -> Stat {
    Stat {
        size: 120,
        modified_seconds: 1_700_000_000,
        modified_nanos: 123,
    }
}

#[test]
fn checks_exact_witness_and_processed_model() {
    let (_directory, mut index) = open();
    let selected = binding();
    index.bind(selected).unwrap();
    let mut document = document("notes.md");
    document.embedded = false;
    index
        .replace(selected, &document, &[part("cached content", None)])
        .unwrap();
    let stat = stamp();
    assert!(
        !index
            .cached(selected, &document.locator, stat, &document.tags, false)
            .unwrap()
    );
    index.remember(selected, &document.locator, stat).unwrap();
    assert!(
        index
            .cached(selected, &document.locator, stat, &document.tags, false)
            .unwrap()
    );
    assert!(
        !index
            .cached(selected, &document.locator, stat, &document.tags, true)
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
    ] {
        assert!(
            !index
                .cached(selected, &document.locator, changed, &document.tags, false)
                .unwrap()
        );
    }
    assert!(
        !index
            .cached(selected, "other.md", stat, &document.tags, false)
            .unwrap()
    );
    assert!(
        !index
            .cached(selected, &document.locator, stat, &["other".into()], false)
            .unwrap()
    );
    let changed = Binding {
        generation: 2,
        ..selected
    };
    assert!(
        !index
            .cached(changed, &document.locator, stat, &document.tags, false)
            .unwrap()
    );
    assert_eq!(
        index
            .remember(changed, &document.locator, stat)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    index.bind(changed).unwrap();
    assert!(
        index
            .cached(changed, &document.locator, stat, &document.tags, false)
            .unwrap()
    );
    document.embedded = true;
    index
        .replace(
            changed,
            &document,
            &[part("cached content", Some(&[1., 0.]))],
        )
        .unwrap();
    assert!(
        !index
            .cached(changed, &document.locator, stat, &document.tags, true)
            .unwrap()
    );
    index.remember(changed, &document.locator, stat).unwrap();
    assert!(
        index
            .cached(changed, &document.locator, stat, &document.tags, true)
            .unwrap()
    );
    let model = Binding {
        generation: 3,
        model: Some((Model::GraniteMulti, 3)),
        ..changed
    };
    index.bind(model).unwrap();
    assert!(
        !index
            .cached(model, &document.locator, stat, &document.tags, true)
            .unwrap()
    );
    assert!(
        index
            .cached(model, &document.locator, stat, &document.tags, false)
            .unwrap()
    );
    index.prune(model, &[]).unwrap();
    assert_eq!(
        index
            .db
            .query_row("SELECT count(*) FROM file_stamps", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn fills_missing_derived_table_without_losing_sources() {
    let (directory, mut index) = open();
    let selected = binding();
    index.bind(selected).unwrap();
    let document = document("notes.md");
    index
        .replace(
            selected,
            &document,
            &[part("cached content", Some(&[1., 0.]))],
        )
        .unwrap();
    index.db.execute_batch("DROP TABLE file_stamps").unwrap();
    drop(index);
    let index = Index::open(&directory.path().join("knowledge.db")).unwrap();
    assert_eq!(index.counts(selected).unwrap(), (1, 1));
    assert!(index.unchanged(selected, &document).unwrap());
    assert!(
        !index
            .cached(selected, &document.locator, stamp(), &document.tags, true)
            .unwrap()
    );
    index
        .remember(selected, &document.locator, stamp())
        .unwrap();
    assert!(
        index
            .cached(selected, &document.locator, stamp(), &document.tags, true)
            .unwrap()
    );
    assert_eq!(
        index
            .db
            .query_row("SELECT version FROM metadata", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
