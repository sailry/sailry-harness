use super::*;
use serde_json::json;
mod keywords;
mod ranked;

async fn database() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch("CREATE TABLE plugin_values(name TEXT, key TEXT, revision INTEGER, value BLOB, index_data BLOB, PRIMARY KEY(name,key));").await.unwrap();
    db
}

fn metadata() -> Index {
    Index {
        fields: ["literal".into(), String::new()],
        tags: vec!["visible".into()],
        order: 1,
        source: None,
    }
}
fn query() -> Search {
    Search {
        terms: Vec::new(),
        query: None,
        mode: storage::Mode::Keyword,
        weights: [3, 1],
        all: Vec::new(),
        any: Vec::new(),
        offset: 0,
        limit: 100,
    }
}

async fn insert(db: &Connection, key: &str, value: &Value, index: &Index) {
    validate_index(index).unwrap();
    let body = serde_json::to_vec(value).unwrap();
    super::super::write(db, "arbitrary-package", key, value, &body, 0, Some(index))
        .await
        .unwrap();
}

#[tokio::test]
async fn reads_preserve_index_errors() {
    let db = database().await;
    assert!(
        read(&db, "arbitrary-package", &query())
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    let table = table("arbitrary-package");
    assert!(!exists(&db, &table).await.unwrap());
    insert(&db, "one", &json!(true), &metadata()).await;
    db.execute(&format!("UPDATE {table} SET key='wrong'"), ())
        .await
        .unwrap();
    assert_eq!(
        read(&db, "arbitrary-package", &query())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Internal
    );
    db.execute_batch(&format!("DROP TABLE {table}"))
        .await
        .unwrap();
    assert_eq!(
        read(&db, "arbitrary-package", &query())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Internal
    );
    assert!(!exists(&db, &table).await.unwrap());
}

#[tokio::test]
async fn bounds_index_metadata_and_query_inputs() {
    let mut index = metadata();
    index.fields[0] = "x".repeat(storage::MAX_INDEX_BYTES);
    assert!(validate_index(&index).is_err());
    index = metadata();
    index.tags = vec!["tag".into(); storage::MAX_TAGS + 1];
    assert!(validate_index(&index).is_err());
    index = metadata();
    index.tags = vec!["x".repeat(storage::MAX_TAG_BYTES + 1)];
    assert!(validate_index(&index).is_err());
    index = metadata();
    index.fields[1] = "null\0token".into();
    assert!(validate_index(&index).is_err());
    for valid in [
        Search {
            terms: vec!["term".into(); 257],
            ..query()
        },
        Search {
            terms: vec!["x".repeat(8193)],
            ..query()
        },
        Search {
            query: Some("x".repeat(8193)),
            ..query()
        },
        Search {
            limit: u16::MAX,
            ..query()
        },
        Search {
            offset: storage::MAX_NAMESPACE_KEYS as u16 + 1,
            ..query()
        },
        Search {
            all: vec!["tag".into(); 17],
            any: vec!["x".repeat(129)],
            ..query()
        },
    ] {
        validate_query(&valid).unwrap();
    }
    for invalid in [
        Search {
            query: Some("original\0query".into()),
            ..query()
        },
        Search {
            terms: vec!["token\0".into()],
            ..query()
        },
        Search {
            weights: [0, 1],
            ..query()
        },
        Search {
            limit: 0,
            ..query()
        },
    ] {
        assert_eq!(
            read(&database().await, "arbitrary-package", &invalid)
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
}

#[tokio::test]
async fn pages_bound_count_and_encoded_size() {
    let db = database().await;
    let value = json!("x".repeat(240 * 1024));
    for key in ["one", "two", "three"] {
        insert(&db, key, &value, &metadata()).await;
    }
    db.execute("CREATE TABLE markers(id INTEGER PRIMARY KEY)", ())
        .await
        .unwrap();
    db.execute("BEGIN CONCURRENT", ()).await.unwrap();
    let first = read(&db, "arbitrary-package", &query()).await.unwrap();
    assert!(!db.is_autocommit().unwrap());
    db.execute("INSERT INTO markers VALUES(1)", ())
        .await
        .unwrap();
    db.execute("COMMIT", ()).await.unwrap();
    assert_eq!(first.entries.len(), 2);
    assert_eq!(first.next, Some(2));
    let encoded = serde_json::to_vec(&sailry_protocol::plugin::transaction::sdk::output(
        Output::PluginSearch(first.clone()),
    ))
    .unwrap();
    assert!(encoded.len() <= storage::MAX_SEARCH_BYTES);
    let last = read(
        &db,
        "arbitrary-package",
        &Search {
            offset: first.next.unwrap(),
            ..query()
        },
    )
    .await
    .unwrap();
    assert_eq!(last.entries.len(), 1);
    assert_eq!(last.next, None);
    db.execute("BEGIN CONCURRENT", ()).await.unwrap();
    let page = read(
        &db,
        "arbitrary-package",
        &Search {
            limit: 1,
            ..query()
        },
    )
    .await
    .unwrap();
    assert!(!db.is_autocommit().unwrap());
    db.execute("INSERT INTO markers VALUES(2)", ())
        .await
        .unwrap();
    db.execute("COMMIT", ()).await.unwrap();
    let markers = db
        .prepare("SELECT count(*) FROM markers")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<i64>(0)
        .unwrap();
    assert_eq!(markers, 2);
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.next, Some(1));
}

#[tokio::test]
async fn namespace_quota_includes_retained_index_fields() {
    let db = database().await;
    // Leave room for a plain value but not its separately retained searchable fields.
    db.execute(
        "INSERT INTO plugin_values VALUES('arbitrary-package','fill',1,zeroblob(?1),NULL)",
        [storage::MAX_NAMESPACE_BYTES as i64 - 100],
    )
    .await
    .unwrap();
    let value = json!(null);
    let body = serde_json::to_vec(&value).unwrap();
    let mut index = metadata();
    index.fields[0] = "x".repeat(128);
    assert_eq!(
        super::super::write(
            &db,
            "arbitrary-package",
            "indexed",
            &value,
            &body,
            0,
            Some(&index)
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Busy
    );
    assert!(
        !super::super::read(&db, "arbitrary-package", "indexed")
            .await
            .unwrap()
            .present
    );
    super::super::write(&db, "arbitrary-package", "ordinary", &value, &body, 0, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn bounds_output_without_rejecting_requested_pages() {
    let db = database().await;
    for number in 0..101 {
        insert(
            &db,
            &format!("item-{number:03}"),
            &json!(number),
            &metadata(),
        )
        .await;
    }
    let first = read(
        &db,
        "arbitrary-package",
        &Search {
            limit: u16::MAX,
            ..query()
        },
    )
    .await
    .unwrap();
    assert_eq!(first.entries.len(), 100);
    assert_eq!(first.next, Some(100));
    let last = read(
        &db,
        "arbitrary-package",
        &Search {
            limit: u16::MAX,
            offset: first.next.unwrap(),
            ..query()
        },
    )
    .await
    .unwrap();
    assert_eq!(last.entries.len(), 1);
    assert_eq!(last.next, None);
    assert_ne!(first.entries.last().unwrap().key, last.entries[0].key);
}
