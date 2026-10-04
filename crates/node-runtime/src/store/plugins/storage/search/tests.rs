use super::*;
use serde_json::json;

fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE plugin_values(name TEXT, key TEXT, revision INTEGER, value BLOB, index_data BLOB, PRIMARY KEY(name,key));").unwrap();
    db
}

fn metadata() -> Index {
    Index {
        fields: ["literal".into(), String::new()],
        tags: vec!["visible".into()],
        order: 1,
    }
}
fn query() -> Search {
    Search {
        terms: Vec::new(),
        weights: [3, 1],
        all: Vec::new(),
        any: Vec::new(),
        offset: 0,
        limit: 100,
    }
}

fn insert(db: &Connection, key: &str, value: &Value, index: &Index) {
    validate_index(index).unwrap();
    let body = serde_json::to_vec(value).unwrap();
    super::super::write(db, "arbitrary-package", key, value, &body, 0, Some(index)).unwrap();
}

#[test]
fn reads_preserve_index_errors() {
    let db = database();
    assert!(
        read(&db, "arbitrary-package", &query())
            .unwrap()
            .entries
            .is_empty()
    );
    let table = table("arbitrary-package");
    assert!(!exists(&db, &table).unwrap());
    insert(&db, "one", &json!(true), &metadata());
    db.execute(&format!("UPDATE {table} SET key='wrong'"), [])
        .unwrap();
    assert_eq!(
        read(&db, "arbitrary-package", &query()).unwrap_err().code,
        ErrorCode::Internal
    );
    db.execute_batch(&format!("DROP TABLE {table}")).unwrap();
    assert_eq!(
        read(&db, "arbitrary-package", &query()).unwrap_err().code,
        ErrorCode::Internal
    );
    assert!(!exists(&db, &table).unwrap());
}

#[test]
fn bounds_index_metadata_and_query_inputs() {
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
    for invalid in [
        Search {
            terms: vec!["term".into(); storage::MAX_TERMS + 1],
            ..query()
        },
        Search {
            terms: vec!["x".repeat(storage::MAX_QUERY_BYTES + 1)],
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
            limit: storage::MAX_PAGE_LIMIT + 1,
            ..query()
        },
        Search {
            limit: 0,
            ..query()
        },
        Search {
            all: vec!["tag".into(); storage::MAX_TAGS + 1],
            ..query()
        },
        Search {
            any: vec!["x".repeat(storage::MAX_TAG_BYTES + 1)],
            ..query()
        },
        Search {
            offset: storage::MAX_NAMESPACE_KEYS as u16 + 1,
            ..query()
        },
    ] {
        assert_eq!(
            read(&database(), "arbitrary-package", &invalid)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
}

#[test]
fn caps_count_and_encoded_output_before_returning_a_page() {
    let db = database();
    let value = json!("x".repeat(240 * 1024));
    for key in ["one", "two", "three"] {
        insert(&db, key, &value, &metadata());
    }
    let first = read(&db, "arbitrary-package", &query()).unwrap();
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
    .unwrap();
    assert_eq!(last.entries.len(), 1);
    assert_eq!(last.next, None);
    let page = read(
        &db,
        "arbitrary-package",
        &Search {
            limit: 1,
            ..query()
        },
    )
    .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.next, Some(1));
}

#[test]
fn namespace_quota_includes_retained_index_fields() {
    let db = database();
    // Leave room for a plain value but not its separately retained searchable fields.
    db.execute(
        "INSERT INTO plugin_values VALUES('arbitrary-package','fill',1,zeroblob(?1),NULL)",
        [storage::MAX_NAMESPACE_BYTES as i64 - 100],
    )
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
        .unwrap_err()
        .code,
        ErrorCode::Busy
    );
    assert!(
        !super::super::read(&db, "arbitrary-package", "indexed")
            .unwrap()
            .present
    );
    super::super::write(&db, "arbitrary-package", "ordinary", &value, &body, 0, None).unwrap();
}
